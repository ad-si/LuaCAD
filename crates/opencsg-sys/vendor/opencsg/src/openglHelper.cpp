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
// openglHelper.h
//

#include "opencsgConfig.h"
#include "openglExt.h"
#include "openglHelper.h"

namespace OpenCSG {

    namespace OpenGL {

        GLfloat projection[16];
        GLfloat modelview[16];
        GLint canvasPos[4];

        GLint stencilBits = 0;
        unsigned int stencilMax = 0;
        unsigned int stencilMask = 0;

        GLint scissorPos[4];

        void scissor(const PCArea& area) {
            const int dx = area.maxx - area.minx;
            const int dy = area.maxy - area.miny;
            glScissor(area.minx, area.miny, dx, dy);
            glEnable(GL_SCISSOR_TEST);
        }

        void drawQuad() {
            glMatrixMode(GL_PROJECTION);
            glPushMatrix();
            glLoadIdentity();
            glMatrixMode(GL_MODELVIEW);
            glPushMatrix();
            glLoadIdentity();

            const GLfloat v[8] = {
                  -1.0f, -1.0f,
                   1.0f, -1.0f,
                  -1.0f,  1.0f,
                   1.0f,  1.0f
            };

            GLboolean origVertexArrayState = glIsEnabled(GL_VERTEX_ARRAY);
            if (!origVertexArrayState) {
                glEnableClientState(GL_VERTEX_ARRAY);
            }

            glVertexPointer(2, GL_FLOAT, 0, v);
            glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);

            if (!origVertexArrayState) {
                glDisableClientState(GL_VERTEX_ARRAY);
            }

            glMatrixMode(GL_PROJECTION);
            glPopMatrix();
            glMatrixMode(GL_MODELVIEW);
            glPopMatrix();
        }

    } // namespace OpenGL

} // namespace OpenCSG
