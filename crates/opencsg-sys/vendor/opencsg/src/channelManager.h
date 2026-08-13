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
// channelManager.h
//
// manage resources for temporary CSG results
//

#ifndef __OpenCSG__channel_manager_h__
#define __OpenCSG__channel_manager_h__

#include "opencsgConfig.h"
#include "openglExt.h"
#include <utility>
#include <vector>

namespace OpenCSG {

    namespace OpenGL {
        class OffscreenBuffer;
    }

    class Primitive;

    enum Channel {
        NoChannel = 0, Alpha = 1, Red = 2, Green = 4, Blue = 8, AllChannels = 15
    };

    enum ProjTextureSetup {
        FixedFunction = 0, ARBShader = 1, GLSLProgram = 2
    };

    class ChannelManager {
    public:
        /// An offscreen buffer is used to collect CSG results in its four
        /// color channels. These resources are managed in a ChannelManager
        /// object. Since we use one offscreen buffer per OpenCSG context only,
        /// this class is a singleton in practice
        ChannelManager();
        virtual ~ChannelManager();

        /// initializes the ChannelManager object, i.e., creates the offscreen
        /// buffer. Returns false on failure. 
        bool init();

        /// returns a free channel, or NoChannel if nothings available
        Channel find() const;
        /// allocates a new channel for temporary calculation of visibility
        /// information. return NoChannel if no free channel was found.
        virtual Channel request();
        /// returns the currently used channel for visibility calculation
        Channel current() const;
        /// returns channels that currently contain visibility information
        std::vector<Channel> occupied() const;
        /// releases the offscreen buffer and invokes merge() to transfer
        /// the visibility information into the main canvas.
        void free();
        /// transfers visibility information into the main canvas. 
        /// implemented by subclasses SCSChannelManager and 
        /// GoldfeatherChannelManager
        virtual void merge() = 0;

        /// activates or deactivates rendering into a channel
        void renderToChannel(bool on);
        /// Setups texture stuff that makes the size of the offscreen buffer
        /// and the size of the main canvas correspond. Activates texture
        /// containing the content of the offscreen buffer. The actual
        /// workings depends on whether the fixed function pipeline, an
        /// ARB shader, or a GLSL program is used. texSizeInv is the uniform
        /// location of a 2d-vector that is supposed to take the inverse of
        /// the texture size. For non-GLSL, texSizeInv is ignored.
        void setupProjectiveTexture(ProjTextureSetup setup, GLint texSizeInv = -1);
        /// undoes texture settings
        void resetProjectiveTexture(ProjTextureSetup setup);
        /// activate texenv settings such that information in channel is
        /// moved into alpha, to allow alpha testing of the channel.
        static void setupTexEnv(Channel channel);

    protected:
        bool isRectangularTexture() const;

    private:
        ChannelManager(const ChannelManager&);
        ChannelManager& operator=(const ChannelManager&);

        static bool gInUse;

        OpenGL::OffscreenBuffer* mOffscreenBuffer;
        bool mInOffscreenBuffer;

    protected:
        Channel mCurrentChannel;
        int mOccupiedChannels;
    };

    class ChannelManagerForBatches : public ChannelManager {
    public:
        /// Remembers which color channel is used to store visibility 
        /// information of a batch of primitives.
        ChannelManagerForBatches();

        /// allows to remember which primitives are stored in which channel
        /// layer == -1: convex primitives
        /// layer ==  0: frontmost surface
        /// etc ...
        void store(Channel channel, const std::vector<Primitive*>& primitives, int layer);

        /// returns primitives for a channel
        const std::vector<Primitive*> getPrimitives(Channel channel) const;
        /// returns layer for a channel
        int getLayer(Channel channel) const;
        /// clears information
        void clear();

    private:
        ChannelManagerForBatches(const ChannelManagerForBatches&);
        ChannelManagerForBatches& operator=(const ChannelManagerForBatches&);

        std::vector<std::pair<std::vector<Primitive*>, int> > mPrimitives;
    };

} // namespace OpenCSG

#endif // __OpenCSG__channel_manager_h__


