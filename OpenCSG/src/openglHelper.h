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
// trivial OpenGL stuff
//

#ifndef __OpenCSG__opengl_helper_h__
#define __OpenCSG__opengl_helper_h__

#include "opencsgConfig.h"
#include "openglExt.h"
#include "area.h"

namespace OpenCSG {

    namespace OpenGL {

        // copy of the projection matrix during CSG computation
        extern GLfloat projection[16];
        // copy of the modelview matrix during CSG computation
        extern GLfloat modelview[16];
        // copy of the viewport size during CSG computation
        extern GLint canvasPos[4];

        // number of stencil bits in the offscreen buffer
        extern GLint stencilBits;
        // the number where the stencil value would "wrap around" to zero
        extern unsigned int stencilMax;
        // stencilMax - 1
        extern unsigned int stencilMask;

        // copy of the scissor settings for CSG computation
        extern GLint scissorPos[4];

        // enables scissoring into area (given in pixel coordinates)
        void scissor(const PCArea& area);

        // renders a full screen quad
        void drawQuad();

    } // namespace OpenGL

} // namespace OpenCSG

#endif // __OpenCSG__opengl_helper_h__
