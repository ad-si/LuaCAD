// OpenCSG - library for image-based CSG rendering for OpenGL
// Copyright (C) 2010-2026, Florian Kirsch
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
// context.h
//
// OpenCSG context management
//

#ifndef __OpenCSG__context_h__
#define __OpenCSG__context_h__

#include "opencsgConfig.h"
#include <opencsg.h>
#include "openglExt.h"

namespace OpenCSG {

    /// redeclared from opencsg.h
    void setContext(int context);
    /// redeclared from opencsg.h
    int getContext();
    /// redeclared from opencsg.h
    void freeResources();

    namespace OpenGL {

        class OffscreenBuffer;

        /// Retrieves the OpenGL function pointers, including the
        /// functions pointers of OpenGL extensions, from the
        /// OpenGL extension library (such as GLEW or glad).
        void ensureFunctionPointers();

        /// Checks the OpenGL-extensions resp. the current settings
        /// and returns a concrete offscreen buffer, for the currently
        /// active context in OpenCSG.
        OffscreenBuffer* getOffscreenBuffer(OffscreenType type);

        /// Given a constant(!) ARB vertex program string and its length,
        /// returns a ARB vertex program object, for the currently
        /// active context in OpenCSG.
        GLuint getARBVertexProgram(const char* prog, int len);

        /// Given a constant(!) ARB fragment program string and its length,
        /// returns a ARB fragment program object, for the currently
        /// active context in OpenCSG.
        GLuint getARBFragmentProgram(const char* prog, int len);

        GLuint getGLSLVertexShader(const char* prog);

        GLuint getGLSLFragmentShader(const char* prog);

        /// Given a constant(!), null-terminated vertex and fragment
        /// program strings, returns a GLSL program object, for the
        /// currently active context in OpenCSG.
        GLuint getGLSLProgram(const char* programId, const char* vertexShader, const char* fragmentShader);

        /// Frees all resources (offscreen buffers, fragment programs...)
        /// allocated for the currently active context in OpenCSG.
        void freeResources();

    } // namespace OpenGL

} // namespace OpenCSG

#endif // __OpenCSG__context_h__

