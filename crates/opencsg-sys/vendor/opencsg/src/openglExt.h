// OpenCSG - library for image-based CSG rendering for OpenGL
// Copyright (C) 2023-2026, Florian Kirsch
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
// openglExt.h
//
// wrap libraries for extension checking, to ease switching between
//

#ifndef __OpenCSG__opengl_ext_h__
#define __OpenCSG__opengl_ext_h__

#if 0 // OpenGL extension checking via GLEW. This is
      // what was used up to OpenCSG 1.5.1 (without
      // this funny #define, of course). GLEW has its
      // problems. On Linux, it often links against
      // GLX which may be not available on Wayland systems.

#include <GL/glew.h>

#define OPENCSG_HAS_EXT(name) GLEW_ ## name

inline void initExtensionLibrary()
{
    // implemented empty for historic reasons. In the past, it was
    // expected that glewInit() was called from the external
    // application.
}

#else // OpenGL extension checking with glad.
      // To avoid conflicts with other software
      // that maybe as well use glad, the generated
      // sources of glad are hacked here, such that
      // all symbols end up in the OpenCSG namespace.

// The khronos includes several system includes.
// Those must not be put into the OpenCSG namespace.

#include "glad/include/KHR/khrplatform.h"

namespace OpenCSG {

    // Keep the global namespace clean. Maybe should be even moved into the
    // sub-namespace OpenGL.

    // Unfortunately, this requires some manual editing of the
    // generated glad files: glad.cpp must include glad.h as well
    // while the OpenCSG namespace is open. Furthermore, the
    // C name mangling for the glad symbols must be disabled.

    #include "glad/include/glad/gl.h"

    #define OPENCSG_HAS_EXT(name) GLAD_GL_ ## name

    inline void initExtensionLibrary()
    {
        gladLoaderLoadGL();
    }

} // namespace OpenCSG

#endif

#endif // __OpenCSG__opengl_ext_h__
