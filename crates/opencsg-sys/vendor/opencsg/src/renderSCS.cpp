// OpenCSG - library for image-based CSG rendering for OpenGL
// Copyright (C) 2002-2026, Florian Kirsch,
// Hasso-Plattner-Institute at the University of Potsdam, Germany
//
// This library is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, write to the Free Software Foundation, Inc.,
// 51 Franklin Street, Fifth Floor, Boston, MA 02110 - 1301 USA.

//
// renderSCS.cpp
//
// stuff specific for the SCS algorithm
//

#include "opencsgConfig.h"
#include <opencsg.h>
#include "opencsgRender.h"
#include "batch.h"
#include "channelManager.h"
#include "context.h"
#include "occlusionQuery.h"
#include "openglHelper.h"
#include "primitiveHelper.h"
#include "scissorMemo.h"
#include "sequencer.h"
#include "settings.h"

#include <algorithm>
#include <map>
#include <string>

namespace OpenCSG {

    namespace {

        ScissorMemo* scissor;

        struct IdBufferId {
            GLubyte r;
            GLubyte g;
            GLubyte b;
            GLubyte a;
            GLubyte* vec() {
                return &r;
            }
        };

        struct RenderData {
            IdBufferId bufferId;
        };

        std::map<Primitive*, RenderData> gRenderInfo;

        RenderData* getRenderData(Primitive* primitive) {
            RenderData* dta = &(gRenderInfo.find(primitive))->second;
            return dta;
        }

        // Stores Ids in the alpha buffer only -> only 255 primitives are possible
        class SCSChannelManagerAlphaOnly : public ChannelManagerForBatches {
        public:
            virtual void merge();
        };

        void SCSChannelManagerAlphaOnly::merge() {

            ProjTextureSetup setup = FixedFunction;
            setupProjectiveTexture(setup);

            glEnable(GL_ALPHA_TEST);
            glEnable(GL_CULL_FACE);
            glEnable(GL_DEPTH_TEST);
            glDepthFunc(GL_LESS);
            glDepthMask(GL_TRUE);
            glColorMask(GL_FALSE, GL_FALSE, GL_FALSE, GL_FALSE);

            std::vector<Channel> channels = occupied();
            for (std::vector<Channel>::const_iterator c = channels.begin(); c!=channels.end(); ++c) {

                setupTexEnv(*c);
                scissor->recall(*c);
                scissor->enableScissor();

                const std::vector<Primitive*> primitives = getPrimitives(*c);
                for (std::vector<Primitive*>::const_iterator j = primitives.begin(); j != primitives.end(); ++j) {
                    glCullFace((*j)->getOperation() == Intersection ? GL_BACK : GL_FRONT);
                    RenderData* primitiveData = getRenderData(*j);
                    GLubyte id = primitiveData->bufferId.a;

                    // Here is an interesting bug, which happened on an ATI HD4670, but actually
                    // might happen on every hardware. I am not sure whether it can be solved 
                    // correctly.

                    // Problem is that in optimized mode with some compilers (VC6, Visual Studio 2003
                    // in particular), when setting the alpha func as follows:
                    // glAlphaFunc(GL_EQUAL, static_cast<float>(id) / 255.0f);
                    // the division is optimized as multiplication with 1.0f/255.0f.
                    // This is a fine and valid optimization. Unfortunately, the results
                    // are not exactly the same as the direct division for some ids.
                    // Which is apparently what the ATI driver is doing internally.
                    // So with comparison with GL_EQUAL fails. 

                    // Fortunately the OpenGL standard enforces that the mapping of color byte
                    // values to float fragment values be done by division. So if the
                    // solution found below (just working at double precision) proves
                    // to work once, it should work forever, such that a precompiling
                    // lookup table containing the correct alpha reference values is 
                    // not required. However a bad feeling remains.

                    // The SCSChannelManagerFragmentProgram path implemented below should fix this.

                    double alpha = static_cast<double>(id) / 255.0;
                    GLfloat fAlpha = static_cast<float>(alpha);
                    glAlphaFunc(GL_EQUAL, fAlpha);

                    (*j)->render();
                }
            }

            scissor->disableScissor();

            glDisable(GL_ALPHA_TEST);
            glDisable(GL_CULL_FACE);
            glDepthFunc(GL_LEQUAL);

            resetProjectiveTexture(setup);

            clear();
        }

        // Stores Ids in the all components of the color buffer
        // -> in theory, 2^32-1 primitives are possible
        // ARB variant
        class SCSChannelManagerARBProgram : public ChannelManagerForBatches {
        public:
            SCSChannelManagerARBProgram(ProjTextureSetup);
            virtual Channel request();
            virtual void merge();

        private:
            ProjTextureSetup mProjTextureSetup;
        };

        SCSChannelManagerARBProgram::SCSChannelManagerARBProgram(ProjTextureSetup setup)
          : mProjTextureSetup(setup)
        {
        }

        Channel SCSChannelManagerARBProgram::request() {
            ChannelManagerForBatches::request();
            mCurrentChannel = AllChannels;
            mOccupiedChannels = mCurrentChannel;
            return mCurrentChannel;
        }

        // Emulates eye texgen
        static const char mergeARBVertexProgram[] =
"!!ARBvp1.0 OPTION ARB_position_invariant;\n"
"ATTRIB  pos = vertex.position;\n"
"ATTRIB  col = vertex.color;\n"
"OUTPUT  outCol = result.color;\n"
"OUTPUT  outTex0 = result.texcoord[0];\n"
"PARAM   mvpmat[4] = { state.matrix.mvp };\n"
"PARAM   texmat[4] = { state.matrix.texture[0] };\n"
"TEMP    eye;\n"
"TEMP    tex;\n"
"DP4     eye.x, mvpmat[0], pos;\n"
"DP4     eye.y, mvpmat[1], pos;\n"
"DP4     eye.z, mvpmat[2], pos;\n"
"DP4     eye.w, mvpmat[3], pos;\n"
"DP4     tex.x, texmat[0], eye;\n"
"DP4     tex.y, texmat[1], eye;\n"
"DP4     tex.z, texmat[2], eye;\n"
"DP4     tex.w, texmat[3], eye;\n"
"MOV     outTex0, tex;\n"
"MOV     outCol, col;\n"
"END";

        // Subtract color from texture value, takes the absolute value
        // and adds all components into each channel of the result, scaled by 2.0f.
        // This way, all 32-bits of the color channel can be used
        // for an 'equal' alpha test, i.e, to check if value in texture
        // and color are equal.

        // Note that 1.0f/255.0f cannot be the result of the above computation.
        // Either the result is 0 (if all components were equal, disregarding
        // numerical errors), or larger/equal than 2.0f/255.0f. The alpha function
        // then is actually set to  GL_LESS, 1.0f/255.0f. This is robust,
        // in contract to the GL_EQUAL in SCSChannelManagerAlphaOnly above.
        // The scaling by 2.0f is required for NVidia hardware, which considers
        // the alpha function GL_LESS, 0.5f/255.0f as GL_LESS, 0.0f for some reason.
        static const char mergeARBFragmentProgramRect[] =
"!!ARBfp1.0\n"
"TEMP    temp;\n"
"ATTRIB  tex0 = fragment.texcoord[0];\n"
"ATTRIB  col0 = fragment.color;\n"
"PARAM   scaleByTwo = { 2.0, 2.0, 2.0, 2.0 };\n"
"OUTPUT  out = result.color;\n"
"TXP     temp, tex0, texture[0], RECT;\n"
"SUB     temp, temp, col0;\n"
"ABS     temp, temp;\n"
"DP4     out, temp, scaleByTwo;\n"
"END";

        static const char mergeARBFragmentProgram2D[] =
"!!ARBfp1.0\n"
"TEMP    temp;\n"
"ATTRIB  tex0 = fragment.texcoord[0];\n"
"ATTRIB  col0 = fragment.color;\n"
"PARAM   scaleByTwo = { 2.0, 2.0, 2.0, 2.0 };\n"
"OUTPUT  out = result.color;\n"
"TXP     temp, tex0, texture[0], 2D;\n"
"SUB     temp, temp, col0;\n"
"ABS     temp, temp;\n"
"DP4     out, temp, scaleByTwo;\n"
"END";

        void SCSChannelManagerARBProgram::merge()
        {
            if (mProjTextureSetup == ARBShader)
            {
                GLuint vId = OpenGL::getARBVertexProgram(mergeARBVertexProgram, (sizeof(mergeARBVertexProgram) / sizeof(mergeARBVertexProgram[0])) - 1);
                glBindProgramARB(GL_VERTEX_PROGRAM_ARB, vId);
                glEnable(GL_VERTEX_PROGRAM_ARB);
            }

            GLuint fId =
                isRectangularTexture()
                  ? OpenGL::getARBFragmentProgram(mergeARBFragmentProgramRect, (sizeof(mergeARBFragmentProgramRect) / sizeof(mergeARBFragmentProgramRect[0])) - 1)
                  : OpenGL::getARBFragmentProgram(mergeARBFragmentProgram2D, (sizeof(mergeARBFragmentProgram2D) / sizeof(mergeARBFragmentProgram2D[0])) - 1);
            glBindProgramARB(GL_FRAGMENT_PROGRAM_ARB, fId);
            glEnable(GL_FRAGMENT_PROGRAM_ARB);

            setupProjectiveTexture(mProjTextureSetup);

            glEnable(GL_ALPHA_TEST);
            glEnable(GL_CULL_FACE);
            glEnable(GL_DEPTH_TEST);
            glDepthFunc(GL_LESS);
            glDepthMask(GL_TRUE);
            glColorMask(GL_FALSE, GL_FALSE, GL_FALSE, GL_FALSE);

            glAlphaFunc(GL_LESS, 1.0f / 255.0f);

            std::vector<Channel> channels;
            channels.push_back(AllChannels);
            for (std::vector<Channel>::const_iterator c = channels.begin(); c!=channels.end(); ++c) {

                scissor->recall(*c);
                scissor->enableScissor();

                const std::vector<Primitive*> primitives = getPrimitives(*c);
                for (std::vector<Primitive*>::const_iterator j = primitives.begin(); j != primitives.end(); ++j) {
                    glCullFace((*j)->getOperation() == Intersection ? GL_BACK : GL_FRONT);
                    RenderData* primitiveData = getRenderData(*j);
                    GLubyte * id = primitiveData->bufferId.vec();
                    glColor4ubv(id);
                    (*j)->render();
                }
            }

            scissor->disableScissor();

            glDisable(GL_ALPHA_TEST);
            glDisable(GL_CULL_FACE);
            glDepthFunc(GL_LEQUAL);
            glDisable(GL_FRAGMENT_PROGRAM_ARB);

            if (mProjTextureSetup == ARBShader)
            {
                glDisable(GL_VERTEX_PROGRAM_ARB);
            }

            resetProjectiveTexture(mProjTextureSetup);

            clear();
        }

        // Stores Ids in the all components of the color buffer
        // -> in theory, 2^32-1 primitives are possible
        // GLSL variant
        class SCSChannelManagerGLSLProgram : public ChannelManagerForBatches {
        public:
            virtual Channel request();
            virtual void merge();
        };

        Channel SCSChannelManagerGLSLProgram::request() {
            ChannelManagerForBatches::request();
            mCurrentChannel = AllChannels;
            mOccupiedChannels = mCurrentChannel;
            return mCurrentChannel;
        }

        // Subtract color from texture value, and if the result is not 0
        // (including some epsilon, which is less than 1.0/(256*256),
        // discards the fragment.
        // This way, all 32-bits of the color channel can be used
        // for an 'equal' alpha test, i.e, to check if value in texture
        // and color are equal.
        static const char mergeFragmentProgramRect[] =
            "#version 110\n"
            "#extension GL_ARB_texture_rectangle : enable\n"
            "uniform sampler2DRect texture0;\n"
            "uniform vec4 color;\n"
            "void main() {\n"
            "    vec4 temp = texture2DRect(texture0, gl_FragCoord.xy);\n"
            "    temp = temp - color;\n"
            "    if (dot(temp, temp) > 0.000001)\n"
            "        discard;\n"
            "    gl_FragColor = color;\n"
            "}\n";

        static const char mergeFragmentProgram2D[] =
            "#version 110\n"
            "uniform sampler2D texture0;\n"
            "uniform vec2 texSizeInv;\n"
            "uniform vec4 color;\n"
            "void main() {\n"
            "    vec2 texCoord = vec2(gl_FragCoord.x * texSizeInv.x, gl_FragCoord.y * texSizeInv.y);\n"
            "    vec4 temp = texture2D(texture0, texCoord);\n"
            "    temp = temp - color;\n"
            "    if (dot(temp, temp) > 0.000001)\n"
            "        discard;\n"
            "    gl_FragColor = color;\n"
            "}\n";

        void SCSChannelManagerGLSLProgram::merge()
        {
            const int SCSIdOffset = 2;
            const char* programID = getVertexShader() + (isRectangularTexture() ? 1 : 0) + SCSIdOffset;
            GLuint glslProgram =
                isRectangularTexture()
                    ? OpenGL::getGLSLProgram(programID, getVertexShader(), mergeFragmentProgramRect)
                    : OpenGL::getGLSLProgram(programID, getVertexShader(), mergeFragmentProgram2D);

            GLint col = glGetUniformLocation(glslProgram, "color");
            GLint texSizeInv = -1;
            if (!isRectangularTexture())
                texSizeInv = glGetUniformLocation(glslProgram, "texSizeInv");

            glUseProgram(glslProgram);

            ProjTextureSetup setup = GLSLProgram;
            setupProjectiveTexture(setup, texSizeInv);

            glDisable(GL_ALPHA_TEST);
            glEnable(GL_CULL_FACE);
            glEnable(GL_DEPTH_TEST);
            glDepthFunc(GL_LESS);
            glDepthMask(GL_TRUE);
            glColorMask(GL_FALSE, GL_FALSE, GL_FALSE, GL_FALSE);

            std::vector<Channel> channels;
            channels.push_back(AllChannels);
            for (std::vector<Channel>::const_iterator c = channels.begin(); c!=channels.end(); ++c) {

                scissor->recall(*c);
                scissor->enableScissor();

                const std::vector<Primitive*> primitives = getPrimitives(*c);
                for (std::vector<Primitive*>::const_iterator j = primitives.begin(); j != primitives.end(); ++j) {
                    glCullFace((*j)->getOperation() == Intersection ? GL_BACK : GL_FRONT);
                    RenderData* primitiveData = getRenderData(*j);
                    GLubyte * id = primitiveData->bufferId.vec();
                    glUniform4f(col, static_cast<float>(static_cast<double>(id[0]) / 255.0),
                                     static_cast<float>(static_cast<double>(id[1]) / 255.0),
                                     static_cast<float>(static_cast<double>(id[2]) / 255.0),
                                     static_cast<float>(static_cast<double>(id[3]) / 255.0));
                    (*j)->render();
                }
            }

            scissor->disableScissor();

            glDisable(GL_CULL_FACE);
            glDepthFunc(GL_LEQUAL);
            glUseProgram(0);

            resetProjectiveTexture(setup);

            clear();
        }


        ChannelManagerForBatches* getChannelManager() {

            if (GLAD_GL_VERSION_2_0)
            {
                bool useGLSL = getVertexShader() != 0;
                if (useGLSL)
                    return new SCSChannelManagerGLSLProgram;
            }

            // The ARB vertex program path has the following problem:
            // With Nouveau drivers on Linux and an NVidia GTX 710, the ARB_position_invariant
            // option appears to be buggy sometimes, and this causes z-buffer artifacts.
            // On the other hand, historical Intel drives failed to work for
            // fixed-function vertex setup together with ARB vertex programs.
            if (   OPENCSG_HAS_EXT(ARB_vertex_program)
                && OPENCSG_HAS_EXT(ARB_fragment_program)
            ) {
                std::string vendor;
                if (const char * v = (const char*)glGetString(GL_VENDOR))
                    vendor = v;
                bool isIntel = vendor.find("Intel") == 0;

                ProjTextureSetup setup = isIntel ? ARBShader : FixedFunction;
                return new SCSChannelManagerARBProgram(setup);
            }

            // The fallback path, using fixed function OpenGL, has its own problems:
            // Obviously, it does not support more than 255 primitives. More importantly,
            // due to the alpha test and the GL_EQUAL alpha function used here, due
            // to accuracy problems, the alpha test for some IDs may incorrectly fail.
            // Then, some primitives of the CSG share are completely missing.
            return new SCSChannelManagerAlphaOnly;
        }


        class IDGenerator {
        public:
            IDGenerator() : counter(0) {}
            IdBufferId newID() {
                ++counter;
                IdBufferId newId;
                newId.r =  (counter >> 24) & 0xff;
                newId.g =  (counter >> 16) & 0xff;
                newId.b =  (counter >>  8) & 0xff;
                newId.a =  (counter >>  0) & 0xff;
                return newId;
            }

        private:
            unsigned int counter;
        };

        ChannelManagerForBatches* channelMgr;

        void renderIntersectedFront(const std::vector<Primitive*>& primitives) {

            const std::size_t numberOfPrimitives = primitives.size();

            glDepthMask(GL_TRUE);

            // optimization for only one shape
            if (numberOfPrimitives == 1) {
                channelMgr->renderToChannel(true);
                glDepthFunc(GL_GREATER);
                glCullFace(GL_BACK);
                glEnable(GL_CULL_FACE);
                RenderData * primitiveData = getRenderData(primitives[0]);
                GLubyte * id = primitiveData->bufferId.vec();
                glColor4ubv(id);
                primitives[0]->render();
                glDisable(GL_CULL_FACE);
                glDepthFunc(GL_LESS);

                return;
            }

            // draw furthest front face
            channelMgr->renderToChannel(true);
            glStencilMask(OpenGL::stencilMask);
            glDepthFunc(GL_GREATER);
            glCullFace(GL_BACK);
            glEnable(GL_CULL_FACE);

            {
                for (std::vector<Primitive*>::const_iterator i = primitives.begin(); i != primitives.end(); ++i) {
                    RenderData * primitiveData = getRenderData(*i);
                    GLubyte * id = primitiveData->bufferId.vec();
                    glColor4ubv(id);
                    (*i)->render();
                }
            }

            // count back faces behind furthest front face
            channelMgr->renderToChannel(false);
            glStencilFunc(GL_ALWAYS, 0, OpenGL::stencilMask);
            glStencilOp(GL_KEEP, GL_KEEP, GL_INCR);
            glEnable(GL_STENCIL_TEST);
            glDepthMask(GL_FALSE);
            glCullFace(GL_FRONT);

            {
                for (std::vector<Primitive*>::const_iterator i = primitives.begin(); i != primitives.end(); ++i) {
                    (*i)->render();
                }
            }

            // where #back faces behind furthest front face != #intersected shapes
            // ->reset fragment
            channelMgr->renderToChannel(true);
            glStencilFunc(GL_NOTEQUAL, static_cast<GLint>(numberOfPrimitives), OpenGL::stencilMask);
            glDepthFunc(GL_ALWAYS);
            glDepthRange(0.0, 0.0);
            glDepthMask(GL_TRUE);
            glStencilOp(GL_ZERO, GL_ZERO, GL_ZERO);
            glDisable(GL_CULL_FACE);
            glColor4ub(0, 0, 0, 0);

            OpenGL::drawQuad();

            glDepthRange(0.0, 1.0);
            glDepthFunc(GL_LESS);
            glDisable(GL_STENCIL_TEST);
        }

        void subtractPrimitives(const std::vector<Batch>& batches,
                                const unsigned int depthComplexity = 0) {

            int setting = getOption(CameraOutsideOptimization);
            bool cameraInsideModel = (setting == OptimizationOff);

            glStencilMask(OpenGL::stencilMask);
            glEnable(GL_STENCIL_TEST);
            glEnable(GL_CULL_FACE);

            size_t numberOfBatches = batches.size();
            BouncingSequencer bounce(numberOfBatches);
            SchoenfieldSequencer schoenfield(numberOfBatches);
            Sequencer * sequencer = 0;
            size_t numIterations;
            if (depthComplexity == 0)
            {
                sequencer = &schoenfield;
                numIterations = sequencer->size();
            }
            else
            {
                sequencer = &bounce;
                numIterations = sequencer->sizeForDepthComplexity(depthComplexity);
            }

            unsigned int stencilref = 0;
            for (size_t i = 0; i < numIterations; ++i)
            {
                const Batch& batch = batches[sequencer->index(i)];

                // create a distinct reference value
                ++stencilref;
                if (stencilref == OpenGL::stencilMax) {
                    glClear(GL_STENCIL_BUFFER_BIT);
                    stencilref = 1;
                }

                channelMgr->renderToChannel(false);

                glDepthMask(GL_FALSE );
                glStencilFunc(GL_ALWAYS, stencilref, OpenGL::stencilMask);
                glStencilOp(GL_KEEP, GL_KEEP, GL_REPLACE);

                if (cameraInsideModel)
                {
                    glDepthFunc(GL_GREATER);
                    glCullFace(GL_FRONT);

                    for (Batch::const_iterator j = batch.begin(); j != batch.end(); ++j) {
                        (*j)->render();
                    }

                    glStencilFunc(GL_EQUAL, stencilref, OpenGL::stencilMask);
                    glStencilOp(GL_ZERO, GL_ZERO, GL_KEEP);
                }

                glDepthFunc(GL_LESS);
                glCullFace(GL_BACK);

                {
                    for (Batch::const_iterator j = batch.begin(); j != batch.end(); ++j) {
                        (*j)->render();
                    }
                }

                // where front faces have been visible, render back faces
                channelMgr->renderToChannel(true);
                glDepthFunc(GL_GREATER);
                glDepthMask(GL_TRUE);
                glCullFace(GL_FRONT);
                glStencilFunc(GL_EQUAL, stencilref, OpenGL::stencilMask);
                glStencilOp(GL_ZERO, GL_ZERO, GL_ZERO);

                {
                    for (Batch::const_iterator j = batch.begin(); j != batch.end(); ++j) {
                        RenderData * primitiveData = getRenderData(*j);
                        GLubyte * id = primitiveData->bufferId.vec();
                        glColor4ubv(id);
                        (*j)->render();
                    }
                }
            }

            glDisable(GL_STENCIL_TEST);
        }

        bool subtractPrimitivesWithOcclusionQueries(const std::vector<Batch>& batches) {

            OpenGL::OcclusionQuery* occlusionTest = OpenGL::getOcclusionQuery(true);
            if (!occlusionTest) {
                return false;
            }

            const std::size_t numberOfBatches = batches.size();

            int setting = getOption(CameraOutsideOptimization);
            bool cameraInsideModel = (setting == OptimizationOff);

            glStencilMask(OpenGL::stencilMask);
            glEnable(GL_STENCIL_TEST);
            glEnable(GL_CULL_FACE);

            std::vector<unsigned int> fragmentcount(numberOfBatches, 0);
            unsigned int shapesWithoutUpdate = 0;

            SimpleSequencer sequencer(numberOfBatches);
            size_t numIterations = sequencer.size();

            unsigned int stencilref = 0;

            for (size_t i = 0; i < numIterations; ++i)
            {
                size_t idx = sequencer.index(i);
                const Batch& batch = batches[idx];

                // create a distinct reference value
                ++stencilref;
                if (stencilref == OpenGL::stencilMax) {
                    glClear(GL_STENCIL_BUFFER_BIT);
                    stencilref = 1;
                }

                channelMgr->renderToChannel(false);

                glDepthMask(GL_FALSE);
                glStencilFunc(GL_ALWAYS, stencilref, OpenGL::stencilMask);
                glStencilOp(GL_KEEP, GL_KEEP, GL_REPLACE);

                occlusionTest->beginQuery();
                if (cameraInsideModel)
                {
                    glDepthFunc(GL_GREATER);
                    glCullFace(GL_FRONT);

                    {
                        for (Batch::const_iterator j = batch.begin(); j != batch.end(); ++j) {
                            (*j)->render();
                        }
                    }

                    glStencilFunc(GL_EQUAL, stencilref, OpenGL::stencilMask);
                    glStencilOp(GL_ZERO, GL_ZERO, GL_KEEP);
                }

                glDepthFunc(GL_LESS);
                glCullFace(GL_BACK);

                {
                    for (Batch::const_iterator j = batch.begin(); j != batch.end(); ++j) {
                        (*j)->render();
                    }
                }
                occlusionTest->endQuery();
                // the fragment count query could occur here, but benches show that
                // the algorithm is faster if the query is delayed.
                // where front faces have been visible, render back faces
                channelMgr->renderToChannel(true);
                glDepthFunc(GL_GREATER);
                glDepthMask(GL_TRUE);
                glCullFace(GL_FRONT);
                glStencilFunc(GL_EQUAL, stencilref, OpenGL::stencilMask);
                glStencilOp(GL_ZERO, GL_ZERO, GL_ZERO);

                {
                    for (Batch::const_iterator j = batch.begin(); j != batch.end(); ++j) {
                        RenderData * primitiveData = getRenderData(*j);
                        GLubyte * id = primitiveData->bufferId.vec();
                        glColor4ubv(id);
                        (*j)->render();
                    }
                }

                unsigned int newFragmentCount = occlusionTest->getQueryResult();
                if (newFragmentCount != fragmentcount[idx]) {
                    fragmentcount[idx] = newFragmentCount;
                    shapesWithoutUpdate = 0;
                } else {
                    ++shapesWithoutUpdate;
                    if (shapesWithoutUpdate >= numberOfBatches)
                        break;
                }
            }

            delete occlusionTest;

            glDisable(GL_STENCIL_TEST);

            return true;
        }

        void renderIntersectedBack(const std::vector<Primitive*>& primitives) {
            // where a back face of intersected shape is in front of any subtracted shape
            // mask fragment as invisible. Updating depth values is not necessary, so when
            // having IDs, this is kind of simple.
            channelMgr->renderToChannel(true);
            glEnable(GL_CULL_FACE);
            glCullFace(GL_FRONT);
            glDepthMask(GL_FALSE);
            glDepthFunc(GL_LESS);
            glColor4ub(0, 0, 0, 0);

            for (std::vector<Primitive*>::const_iterator i = primitives.begin(); i != primitives.end(); ++i) {
                (*i)->render();
            }

            glDepthMask(GL_TRUE);
        }

    } // unnamed namespace

    void renderSCS(const std::vector<Primitive*>& primitives, DepthComplexityAlgorithm algorithm) {

        channelMgr = getChannelManager();
        if (!channelMgr->init())
        {
            delete channelMgr;
            return;
        }

        gRenderInfo.clear();

        scissor = new ScissorMemo;

        std::vector<Primitive*> intersected; intersected.reserve(primitives.size());
        std::vector<Primitive*> subtracted;  subtracted.reserve(primitives.size());

        {
            IDGenerator IDMaker;
            for (std::vector<Primitive*>::const_iterator itr = primitives.begin(); itr != primitives.end(); ++itr) {
                {
                    RenderData dta; 
                    dta.bufferId = IDMaker.newID();
                    gRenderInfo.insert(std::make_pair(*itr, dta));
                }
                Operation operation= (*itr)->getOperation();
                if (operation == Intersection) {
                    intersected.push_back(*itr);
                } else if (operation == Subtraction) {
                    subtracted.push_back(*itr);
                }
            }
        }

        Batcher subtractedBatches(subtracted);

        scissor->setIntersected(intersected);
        scissor->setCurrent(intersected);

        unsigned int depthComplexity = 0;
        if (algorithm == DepthComplexitySampling) {
            scissor->enableScissor();
            glClear(GL_STENCIL_BUFFER_BIT);
            depthComplexity =
                (std::min)(OpenGL::calcMaxDepthComplexity(subtracted, scissor->getCurrentArea()),
                           static_cast<unsigned int>(subtractedBatches.size()));
        }

        channelMgr->request();
        channelMgr->renderToChannel(true);

        scissor->enableScissor();
        scissor->store(channelMgr->current());

        glDepthMask(GL_TRUE);
        glStencilMask(OpenGL::stencilMask);
        glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
        glClearDepth(0.0);      // near clipping plane! essential for algorithm!
        glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT | GL_STENCIL_BUFFER_BIT);
        glClearDepth(1.0);

        renderIntersectedFront(intersected);
        if (!subtractedBatches.batches().empty())
        {
            scissor->enableDepthBounds();
            switch (algorithm) {
            case OcclusionQuery:
                if (subtractPrimitivesWithOcclusionQueries(subtractedBatches.batches()))
                    break; // success
                // Maybe we just should give up here?
                // fall through
            case NoDepthComplexitySampling:
                subtractPrimitives(subtractedBatches.batches());
                break;
            case DepthComplexitySampling:
                subtractPrimitives(subtractedBatches.batches(), depthComplexity);
                break;
            }
            scissor->disableDepthBounds();
        }
        renderIntersectedBack(intersected);

        scissor->disableScissor();

        channelMgr->store(channelMgr->current(), primitives, 0);
        channelMgr->free();

        delete scissor;
        delete channelMgr;
    }

} // namespace OpenCSG
