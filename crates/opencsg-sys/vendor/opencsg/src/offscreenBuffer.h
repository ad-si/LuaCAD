// OpenCSG - library for image-based CSG rendering for OpenGL
// Copyright (C) 2006-2026, Florian Kirsch
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
// offscreenBuffer.h
//
// offscreen buffer abstraction
//

#ifndef __OpenCSG__offscreen_buffer_h__
#define __OpenCSG__offscreen_buffer_h__

#include "opencsgConfig.h"
#include <opencsg.h>

namespace OpenCSG {

    struct Dimensions
    {
        Dimensions()
            : width(-1), height(1)
        {
        }
        Dimensions(int w, int h)
            : width(w), height(h)
        {
        }
        friend bool operator==(const Dimensions& me, const Dimensions& other)
        {
            return (me.width == other.width) && (me.height == other.height);
        }
        int width;
        int height;
    };

    namespace OpenGL {

        class OffscreenBuffer {

        public: // abstract base class
            OffscreenBuffer() {};
            virtual ~OffscreenBuffer() {};

            /// Reads the current settings, such as the currently bound FBO.
            /// This should be called before every use of the offscreen buffer
            /// even if Initialize() or Resize() do not need to be called.
            virtual bool ReadCurrent() = 0;

            /// Call this once before use, to initialize the offscreen buffer
            /// with the intended width and height.
            virtual bool Initialize(Dimensions dims) = 0;

            /// checks whether Initialize has been called before or not
            virtual bool IsInitialized() const = 0;

            /// Change the size of the render texture. After this, Initialize
            /// must be called again.
            virtual bool Resize(Dimensions dims) = 0;

            /// Begin drawing to the texture. (i.e. use as "output" texture)
            virtual bool BeginCapture() = 0;
            /// End drawing to the texture.
            virtual bool EndCapture() = 0;

            /// Bind the texture to the active texture unit for use as an "input" texture
            virtual void Bind() const = 0;

            /// Enables the texture target appropriate for this render texture.
            virtual void EnableTextureTarget() const = 0;
            /// Disables the texture target appropriate for this render texture.
            virtual void DisableTextureTarget() const = 0;

            /// Returns the texture target this texture is bound to.
            virtual unsigned int GetTextureTarget() const = 0;
            /// Returns the width of the offscreen buffer.
            virtual int GetWidth() const = 0;
            /// Returns the width of the offscreen buffer.
            virtual int GetHeight() const = 0;

        private:
            OffscreenBuffer(const OffscreenBuffer&);
            OffscreenBuffer& operator=(const OffscreenBuffer&);
        };
    } // namespace OpenGL

} // namespace OpenCSG

#endif // __OpenCSG__offscreen_buffer_h__
