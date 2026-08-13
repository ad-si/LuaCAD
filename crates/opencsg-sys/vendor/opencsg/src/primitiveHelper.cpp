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
// primitiveHelper.cpp
//

#include "opencsgConfig.h"
#include <opencsg.h>
#include "openglExt.h"
#include "openglHelper.h"
#include "primitiveHelper.h"
#include <algorithm>

namespace OpenCSG {

    namespace Algo {
        bool intersectXY(const Primitive* a, const Primitive* b) {
            float aminx, aminy, aminz, amaxx, amaxy, amaxz,
                  bminx, bminy, bminz, bmaxx, bmaxy, bmaxz;
            a->getBoundingBox(aminx, aminy, aminz, amaxx, amaxy, amaxz);
            b->getBoundingBox(bminx, bminy, bminz, bmaxx, bmaxy, bmaxz);

            return (bmaxx >= aminx) && (amaxx >= bminx)
                && (bmaxy >= aminy) && (amaxy >= bminy);
        }
    
        bool intersectXYZ(const Primitive* a, const Primitive* b) {
            float aminx, aminy, aminz, amaxx, amaxy, amaxz,
                  bminx, bminy, bminz, bmaxx, bmaxy, bmaxz;
            a->getBoundingBox(aminx, aminy, aminz, amaxx, amaxy, amaxz);
            b->getBoundingBox(bminx, bminy, bminz, bmaxx, bmaxy, bmaxz);

            return (bmaxx >= aminx) && (amaxx >= bminx)
                && (bmaxy >= aminy) && (amaxy >= bminy) 
                && (bmaxz >= aminz) && (amaxz >= bminz);
        }

        bool containsXY(const Primitive* a, const Primitive* b) {
            float aminx, aminy, aminz, amaxx, amaxy, amaxz,
                  bminx, bminy, bminz, bmaxx, bmaxy, bmaxz;
            a->getBoundingBox(aminx, aminy, aminz, amaxx, amaxy, amaxz);
            b->getBoundingBox(bminx, bminy, bminz, bmaxx, bmaxy, bmaxz);

            return (aminx >= bminx) && (amaxx <= bmaxx)
                && (aminy >= bminy) && (amaxy <= bmaxy);
        }

        unsigned int getConvexity(const std::vector<Primitive*>& batch) {
            unsigned int convexity=1;
            for (std::vector<Primitive*>::const_iterator itr = batch.begin(); itr != batch.end(); ++itr) {
                unsigned int val = (*itr)->getConvexity();
                if (convexity < val) {
                    convexity = val;
                }
            }

            return convexity;
        }
    } // namespace Algo

    namespace OpenGL {

        unsigned int calcMaxDepthComplexity(const std::vector<Primitive*>& primitives,
                                            const PCArea& area) {

            glColorMask(GL_FALSE, GL_FALSE, GL_FALSE, GL_FALSE);

            glDisable(GL_DEPTH_TEST);
            glStencilMask(255);
            glEnable(GL_STENCIL_TEST);
            glStencilFunc(GL_ALWAYS, 0, 255);
            glStencilOp(GL_INCR, GL_INCR, GL_INCR);

            glEnable(GL_CULL_FACE);

            for (std::vector<Primitive*>::const_iterator itr = primitives.begin(); itr != primitives.end(); ++itr) {
                glCullFace((*itr)->getOperation() == Intersection ? GL_BACK : GL_FRONT);
                (*itr)->render();
            }

            glDisable(GL_CULL_FACE);
            glDisable(GL_STENCIL_TEST);
            glEnable(GL_DEPTH_TEST);

            int dx = area.maxx - area.minx;
            int dy = area.maxy - area.miny;

            std::size_t size = static_cast<std::size_t>(dx) * dy;
            GLubyte * buf = new GLubyte[size];

            glPixelStorei(GL_PACK_SWAP_BYTES, GL_FALSE);
            glPixelStorei(GL_PACK_ALIGNMENT, 1);
            glPixelStorei(GL_PACK_ROW_LENGTH, 0);
            glPixelStorei(GL_PACK_SKIP_ROWS, 0);
            glPixelStorei(GL_PACK_SKIP_PIXELS, 0);

            // This is pathologically slow on ATI HD4670 if not the complete viewport is read back.
            // So better always read the complete viewport or make this configurable?
            glReadPixels(area.minx, area.miny, dx, dy, GL_STENCIL_INDEX, GL_UNSIGNED_BYTE, buf);

            unsigned char m = *std::max_element(buf, buf+size);

            unsigned int max = m;

            delete[] buf;

            return max;
        }

        void renderLayer(unsigned int layer, const std::vector<Primitive*>& primitives) {
            glStencilFunc(GL_EQUAL, layer, 255);
            glStencilOp(GL_INCR, GL_INCR, GL_INCR);
            glStencilMask(255);
            glEnable(GL_STENCIL_TEST);
  
            glEnable(GL_CULL_FACE);
            for (std::vector<Primitive*>::const_iterator j = primitives.begin(); j != primitives.end(); ++j) {
                glCullFace((*j)->getOperation() == Intersection ? GL_BACK : GL_FRONT);
                (*j)->render();
            }
            glDisable(GL_CULL_FACE);
        }

    } // namespace OpenGL

} // namespace OpenCSG
