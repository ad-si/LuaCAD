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
// channelManager.cpp
//

#include "opencsgConfig.h"
#include "channelManager.h"

#include "context.h"
#include "offscreenBuffer.h"
#include "openglHelper.h"
#include "settings.h"

#include <algorithm>
#include <cassert>

namespace OpenCSG {

    bool ChannelManager::gInUse = false;

    namespace {

        int nextPow2(int value) {
            if(value <= 0) { return 0; }
            int result = 1;
            while(result < value) {
                result <<= 1;
            }
            return result;
        }

        void defaults() {
            glViewport(OpenGL::canvasPos[0], OpenGL::canvasPos[1], OpenGL::canvasPos[2], OpenGL::canvasPos[3]);
            glClearColor(0.0f, 0.0f, 0.0f, 0.0f);
            glClearDepth(1.0);
            glClearStencil(0);
            glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
            glDepthMask(GL_TRUE);
            glStencilMask(0xffffffff);
            glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT | GL_STENCIL_BUFFER_BIT);
            glEnable(GL_DEPTH_TEST);
        }

        template<int FRAMES>
        class MaximumMemorizer {
            int mMax;
            int mSecondMax;
            int mCounter;
        public:
            MaximumMemorizer() : mMax(0), mSecondMax(-1), mCounter(0) { }
            void newValue(int v) {
                if (v>=mMax) {
                    mMax = v;
                    mSecondMax = -1;
                    mCounter = 0;
                } else {
                    if (v>mSecondMax) {
                        mSecondMax = v;
                    }
                    if (++mCounter >= FRAMES) {
                        mMax = mSecondMax;
                        mSecondMax = -1;
                        mCounter = 0;
                    }
                }
            }
            int getMax() const {
                return mMax;
            }
        };

    } // unnamed namespace

    ChannelManager::ChannelManager()
      : mOffscreenBuffer(0)
      , mInOffscreenBuffer(false)
      , mCurrentChannel(NoChannel)
      , mOccupiedChannels(NoChannel)
    {
        glPushAttrib(GL_ALL_ATTRIB_BITS);
        glDisable(GL_LIGHTING);
        glDisable(GL_TEXTURE_1D);
        glDisable(GL_TEXTURE_2D);
        if (   OPENCSG_HAS_EXT(ARB_texture_rectangle)
            || OPENCSG_HAS_EXT(EXT_texture_rectangle)
            || OPENCSG_HAS_EXT(NV_texture_rectangle))
            glDisable(GL_TEXTURE_RECTANGLE_ARB);
        glDisable(GL_TEXTURE_3D); // OpenGL 1.2 - take this as given
        if (OPENCSG_HAS_EXT(ARB_texture_cube_map))
            glDisable(GL_TEXTURE_CUBE_MAP_ARB);
        glDisable(GL_BLEND);

        glGetFloatv(GL_MODELVIEW_MATRIX, OpenGL::modelview);
        glGetFloatv(GL_PROJECTION_MATRIX, OpenGL::projection);
        glGetIntegerv(GL_VIEWPORT, OpenGL::canvasPos);

        if (glIsEnabled(GL_SCISSOR_TEST)) {
            glGetIntegerv(GL_SCISSOR_BOX, OpenGL::scissorPos);
        } else {
            OpenGL::scissorPos[0] = OpenGL::canvasPos[0];
            OpenGL::scissorPos[1] = OpenGL::canvasPos[1];
            OpenGL::scissorPos[2] = OpenGL::canvasPos[2];
            OpenGL::scissorPos[3] = OpenGL::canvasPos[3];
        }
    }

    bool ChannelManager::init() {

        assert(!gInUse);
        if (gInUse)
            return false;
        gInUse = true;

        OffscreenType newOffscreenType = static_cast<OffscreenType>(getOption(OffscreenSetting));

        if (   newOffscreenType == OpenCSG::AutomaticOffscreenType
            || newOffscreenType == OpenCSG::FrameBufferObject
        ) {
            if (OPENCSG_HAS_EXT(ARB_framebuffer_object)) {
                newOffscreenType = OpenCSG::FrameBufferObjectARB;
            }
            else
            if (   OPENCSG_HAS_EXT(EXT_framebuffer_object)
                && OPENCSG_HAS_EXT(EXT_packed_depth_stencil)
            ) {
                newOffscreenType = OpenCSG::FrameBufferObjectEXT;
            }
            else {
                // At least one set of the above OpenGL extensions is required
                return false;
            }
        }

        mOffscreenBuffer = OpenGL::getOffscreenBuffer(newOffscreenType);

        if (!mOffscreenBuffer)
        {
            // Creating the offscreen buffer failed, maybe the OpenGL extension
            // for the specific offscreen buffer type is not supported
            return false;
        }

        if (!mOffscreenBuffer->ReadCurrent())
        {
            return false;
        }

        const int dx = OpenGL::canvasPos[2] - OpenGL::canvasPos[0];
        const int dy = OpenGL::canvasPos[3] - OpenGL::canvasPos[1];

        int tx = dx;
        int ty = dy;
        // We don't need to enlarge the texture to the next largest power-of-two size if:
        // - the ARB_texture_non_power_of_two extension is supported
        // - or any of the texture rectangle extensions is supported
        // Negating this gives the following expression from hell:
        if (   !OPENCSG_HAS_EXT(ARB_texture_non_power_of_two)
            && !OPENCSG_HAS_EXT(ARB_texture_rectangle)
            && !OPENCSG_HAS_EXT(EXT_texture_rectangle)
            && !OPENCSG_HAS_EXT(NV_texture_rectangle)
        ) {
            // blow up the texture to legal power-of-two size :-(
            tx = nextPow2(dx);
            ty = nextPow2(dy);
        }

        // The following implements a heuristic that makes the offscreen buffer
        // smaller if the size of the buffer has been bigger than necessary
        // in x- or y- direction for resizeOffscreenBufferLimit frames. 
        //
        // this permits to use OpenCSG for CSG rendering in different
        // canvases with different sizes without permanent expensive
        // resizing of the offscreen buffer for every frame.
        //
        // possible improvements: 
        //   - allow the user to define the resizeOffscreenBufferLimit?
        static const unsigned int resizeOffscreenBufferLimit = 64;

        static MaximumMemorizer<resizeOffscreenBufferLimit> sizeX;
        static MaximumMemorizer<resizeOffscreenBufferLimit> sizeY;
        // tx == ty == 0 happens if the window is minimized, in this case don't touch a thing
        if (tx != 0 && ty != 0) {
            sizeX.newValue(tx);
            sizeY.newValue(ty);
        }

        bool rebuild = false;

        Dimensions dims(sizeX.getMax(), sizeY.getMax());
        if (!mOffscreenBuffer->IsInitialized())
        {
            if (!mOffscreenBuffer->Initialize(dims)) {
                // Initializing the offscreen buffer failed, maybe the OpenGL extension
                // for the specific offscreen buffer type is not supported
                return false;
            }
            rebuild = true;
        }
        // tx == ty == 0 happens if the window is minimized, in this case don't touch a thing
        else if (tx != 0 && ty != 0 &&
                    (   mOffscreenBuffer->GetWidth() != dims.width
                     || mOffscreenBuffer->GetHeight() != dims.height
                )   )
        {
            if (!mOffscreenBuffer->Resize(dims)) {
                // Resizing the offscreen buffer failed, maybe the OpenGL extension
                // for the specific offscreen buffer type is not supported. More
                // likely this is a programming error in Resize().
                return false;
            }
            rebuild = true;
        }

        if (rebuild) {
            // assert(gOffscreenBuffer->HasStencil());
            mOffscreenBuffer->BeginCapture();
            defaults();
            glGetIntegerv(GL_STENCIL_BITS, &OpenGL::stencilBits);
            OpenGL::stencilMax = 1 << OpenGL::stencilBits;
            OpenGL::stencilMask = OpenGL::stencilMax - 1;
            mOffscreenBuffer->EndCapture();
            mOffscreenBuffer->Bind();
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        }

        mInOffscreenBuffer = false;
        mCurrentChannel = NoChannel;
        mOccupiedChannels = NoChannel;

        return true;
    }

    ChannelManager::~ChannelManager() {
        glPopAttrib();
        assert(gInUse);
        gInUse = false;
    }

    Channel ChannelManager::find() const {

        Channel channel = NoChannel;

        // find free channel
        if ((mOccupiedChannels & Alpha) == 0) {
            channel = Alpha;
        }  else if (OPENCSG_HAS_EXT(ARB_texture_env_dot3) || GLAD_GL_VERSION_1_3) {
            if ((mOccupiedChannels & Red) == 0)   {
                channel = Red;
            } else if ((mOccupiedChannels & Green) == 0) {
                channel = Green;
            } else if ((mOccupiedChannels & Blue) == 0)  {
                channel = Blue;
            }
        }

        return channel;
    }

    Channel ChannelManager::request() {
        if (!mInOffscreenBuffer) {
            mOffscreenBuffer->BeginCapture();

            mInOffscreenBuffer = true;

            mCurrentChannel = NoChannel;
            mOccupiedChannels = NoChannel;
        }

        mCurrentChannel = find();
        mOccupiedChannels |= mCurrentChannel;
        return mCurrentChannel;
    }

    Channel ChannelManager::current() const {
        return mCurrentChannel;
    }

    std::vector<Channel> ChannelManager::occupied() const {

        std::vector<Channel> result;
        result.reserve(4);

        if ((mOccupiedChannels & Alpha) != 0) {
            result.push_back(Alpha);
        }
        if ((mOccupiedChannels & Red) != 0) {
            result.push_back(Red);
        }
        if ((mOccupiedChannels & Green) != 0) {
            result.push_back(Green);
        }
        if ((mOccupiedChannels & Blue) != 0) {
            result.push_back(Blue);
        }

        return result;
    }

    void ChannelManager::free() {
        if (mInOffscreenBuffer) {
            mOffscreenBuffer->EndCapture();
            mInOffscreenBuffer = false;
        }

        merge();
    }

    void ChannelManager::renderToChannel(bool on) {

        if (on) {
            switch (mCurrentChannel) {
                case NoChannel:
                    glColorMask(GL_FALSE, GL_FALSE, GL_FALSE, GL_FALSE);
                    break;
                case Alpha:
                    glColorMask(GL_FALSE, GL_FALSE, GL_FALSE, GL_TRUE);
                    break;
                case Blue:
                    glColorMask(GL_FALSE, GL_FALSE, GL_TRUE, GL_FALSE);
                    break;
                case Green:
                    glColorMask(GL_FALSE, GL_TRUE, GL_FALSE, GL_FALSE);
                    break;
                case Red:
                    glColorMask(GL_TRUE, GL_FALSE, GL_FALSE, GL_FALSE);
                    break;
                case AllChannels:
                    glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
                    break;
                }
            }
        else {
            glColorMask(GL_FALSE, GL_FALSE, GL_FALSE, GL_FALSE);
        }
    }


    void ChannelManager::setupProjectiveTexture(ProjTextureSetup setup, GLint texSizeInv)
    {
        mOffscreenBuffer->Bind();
        mOffscreenBuffer->EnableTextureTarget();

        if (setup == FixedFunction)
        {
            static const float splane[4] = { 1.0f, 0.0f, 0.0f, 0.0f };
            static const float tplane[4] = { 0.0f, 1.0f, 0.0f, 0.0f };
            static const float rplane[4] = { 0.0f, 0.0f, 1.0f, 0.0f };
            static const float qplane[4] = { 0.0f, 0.0f, 0.0f, 1.0f };

            glTexGeni(GL_S, GL_TEXTURE_GEN_MODE, GL_EYE_LINEAR);
            glTexGeni(GL_T, GL_TEXTURE_GEN_MODE, GL_EYE_LINEAR);
            glTexGeni(GL_R, GL_TEXTURE_GEN_MODE, GL_EYE_LINEAR);
            glTexGeni(GL_Q, GL_TEXTURE_GEN_MODE, GL_EYE_LINEAR);
            glTexGenfv(GL_S, GL_EYE_PLANE, splane);
            glTexGenfv(GL_T, GL_EYE_PLANE, tplane);
            glTexGenfv(GL_R, GL_EYE_PLANE, rplane);
            glTexGenfv(GL_Q, GL_EYE_PLANE, qplane);
            glEnable(GL_TEXTURE_GEN_S);
            glEnable(GL_TEXTURE_GEN_T);
            glEnable(GL_TEXTURE_GEN_R);
            glEnable(GL_TEXTURE_GEN_Q);
        }

        if (setup == FixedFunction || setup == ARBShader)
        {
            glMatrixMode(GL_TEXTURE);

            const int dx = OpenGL::canvasPos[2] - OpenGL::canvasPos[0];
            const int dy = OpenGL::canvasPos[3] - OpenGL::canvasPos[1];

            // with ARB_texture_rectangle texture coordinates range between
            // 0 and dx resp. dy
            float factorX = static_cast<float>(dx);
            float factorY = static_cast<float>(dy);

            // Otherwise, if the texture rectangle extension is not used:
            // Do not check for the non-power-of-two extension, but simply for
            // the texture format. This seems safer, since it should work always.
            if (!isRectangularTexture()) {
                // with ordinary pow-of-two texture coordinates are between 0 and 1
                // but we must assure only the used part of the texture is taken.
                factorX /= static_cast<float>(mOffscreenBuffer->GetWidth());
                factorY /= static_cast<float>(mOffscreenBuffer->GetHeight());
            }

            float   texCorrect[16] = { factorX, 0.0f, 0.0f, 0.0f,
                                       0.0f, factorY, 0.0f, 0.0f,
                                       0.0f,    0.0f, 1.0f, 0.0f,
                                       0.0f,    0.0f, 0.0f, 1.0f };

            static const float p2ndc[16] = { 0.5f, 0.0f, 0.0f, 0.0f,
                                             0.0f, 0.5f, 0.0f, 0.0f,
                                             0.0f, 0.0f, 0.5f, 0.0f,
                                             0.5f, 0.5f, 0.5f, 1.0f };
            glPushMatrix();
            glLoadMatrixf(texCorrect);
            glMultMatrixf(p2ndc);
            if (setup == FixedFunction)
            {
                glMultMatrixf(OpenGL::projection);
                glMultMatrixf(OpenGL::modelview);
            }
            glMatrixMode(GL_MODELVIEW);
        }
        else if (setup == GLSLProgram)
        {
            // If the texture rectangle extension is not used, the uniform
            // with the inverse texture size must be set. The value is used
            // in the 2d fragment shader to map pixel coordinates to texture
            // coordinates between 0 and 1.
            if (!isRectangularTexture())
            {
                glUniform2f(texSizeInv, 1.0f / (std::max)(mOffscreenBuffer->GetWidth(), 1),
                                        1.0f / (std::max)(mOffscreenBuffer->GetHeight(), 1));
            }
        }
    }

    void ChannelManager::resetProjectiveTexture(ProjTextureSetup setup)
    {
        if (setup == FixedFunction)
        {
            glDisable(GL_TEXTURE_GEN_S);
            glDisable(GL_TEXTURE_GEN_T);
            glDisable(GL_TEXTURE_GEN_R);
            glDisable(GL_TEXTURE_GEN_Q);
        }

        if (setup == FixedFunction || setup == ARBShader)
        {
            glMatrixMode(GL_TEXTURE);
            glPopMatrix();
            glMatrixMode(GL_MODELVIEW);
        }

        mOffscreenBuffer->DisableTextureTarget();
    }

    void ChannelManager::setupTexEnv(Channel channel) {

        if (channel == Alpha) {
            glTexEnvi(GL_TEXTURE_ENV, GL_TEXTURE_ENV_MODE, GL_REPLACE);
        } else {
            // replicate color into alpha
            if (OPENCSG_HAS_EXT(ARB_texture_env_dot3) || GLAD_GL_VERSION_1_3) {
                switch (channel) {
                case Red: 
                    glColor3f(1.0f, 0.5f, 0.5f); 
                    break;                
                case Green: 
                    glColor3f(0.5f, 1.0f, 0.5f); 
                    break;
                case Blue: 
                    glColor3f(0.5f, 0.5f, 1.0f); 
                    break;
                default:
                    // should not happen!
                    assert(0);
                }
            } else {
                // should not happen!
                assert(0);
            }

            glTexEnvi(GL_TEXTURE_ENV, GL_TEXTURE_ENV_MODE, GL_COMBINE);
            glTexEnvi(GL_TEXTURE_ENV, GL_COMBINE_RGB, GL_DOT3_RGBA);
            glTexEnvi(GL_TEXTURE_ENV, GL_SOURCE0_RGB, GL_TEXTURE);
            glTexEnvi(GL_TEXTURE_ENV, GL_OPERAND0_RGB, GL_SRC_COLOR);
            glTexEnvi(GL_TEXTURE_ENV, GL_SOURCE1_RGB, GL_PRIMARY_COLOR);
            glTexEnvi(GL_TEXTURE_ENV, GL_OPERAND1_RGB, GL_SRC_COLOR);
        }
    }

    bool ChannelManager::isRectangularTexture() const
    {
        return mOffscreenBuffer->GetTextureTarget() != GL_TEXTURE_2D;
    }




    ChannelManagerForBatches::ChannelManagerForBatches() : 
        ChannelManager(), 
        mPrimitives(std::vector<std::pair<std::vector<Primitive*>, int> >(AllChannels + 1)) {
    }

    void ChannelManagerForBatches::store(Channel channel, const std::vector<Primitive*>& primitives, int layer) {
        mPrimitives[channel] = std::make_pair(primitives, layer);
    }

    const std::vector<Primitive*> ChannelManagerForBatches::getPrimitives(Channel channel) const {
        return mPrimitives[channel].first;
    }

    int ChannelManagerForBatches::getLayer(Channel channel) const {
        return mPrimitives[channel].second;
    }

    void ChannelManagerForBatches::clear() {
        mPrimitives = std::vector<std::pair<std::vector<Primitive*>, int> >(AllChannels + 1);
    }

} // namespace OpenCSG

