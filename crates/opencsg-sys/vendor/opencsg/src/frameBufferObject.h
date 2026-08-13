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
// frameBufferObject.h
//
// frame buffer object class implementing the offscreen buffer interface
//

#ifndef __OpenCSG__frame_buffer_object_h__
#define __OpenCSG__frame_buffer_object_h__

#include "opencsgConfig.h"
#include "offscreenBuffer.h"
#include "openglExt.h"

namespace OpenCSG {

    namespace OpenGL {

        class FrameBufferObject : public OffscreenBuffer {
        public:
            /// ctor / dtor
            FrameBufferObject();
            virtual ~FrameBufferObject();

            /// Reads the currently bound FBO.
            virtual bool ReadCurrent();

            /// Initializes the frame buffer object with the intended width and height.
            /// The frame buffer object is created with RGBA and combined depth/stencil buffer.
            virtual bool Initialize(Dimensions dims);

            /// checks whether Initialize has been called before or not
            virtual bool IsInitialized() const { return initialized; }

            /// Change the size of the frame buffer object.
            virtual bool Resize(Dimensions dims);

            /// Begin drawing to the frame buffer object. (i.e. use as "output" texture)
            virtual bool BeginCapture();
            /// End drawing to the frame buffer object.
            virtual bool EndCapture();

            /// Bind the frame buffer object to the active texture unit for use as an "input" texture
            virtual void Bind() const;

            /// Enables the texture target appropriate for this frame buffer object.
            virtual void EnableTextureTarget() const { if (initialized) glEnable(textureTarget); }
            /// Disables the texture target appropriate for this frame buffer object.
            virtual void DisableTextureTarget() const { if (initialized) glDisable(textureTarget); }

            /// Returns the texture target this texture is bound to.
            virtual unsigned int GetTextureTarget() const { return textureTarget; }
            /// Returns the width of the frame buffer object.
            virtual int GetWidth() const  { return dimensions.width;  }
            /// Returns the width of the frame buffer object.
            virtual int GetHeight() const { return dimensions.height; }

        private:
            FrameBufferObject(const FrameBufferObject&);
            FrameBufferObject& operator=(const FrameBufferObject&);

            // Width and height of the frame buffer object
            Dimensions   dimensions;

            /// Texture stuff
            GLenum       textureTarget;
            unsigned int textureID;
            unsigned int depthID;

            unsigned int framebufferID;
            int          oldFramebufferID;

            bool         initialized;

            /// Removes the frame buffer object OpenGL resources.
            bool Reset();
        };

    } // namespace OpenGL

} // namespace OpenCSG

#endif // __OpenCSG__frame_buffer_object_h__
