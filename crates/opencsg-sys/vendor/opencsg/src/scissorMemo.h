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
// scissorMemo.h
//
// handles scissor calculations
//

#ifndef __OpenCSG__scissor_memo_h__
#define __OpenCSG__scissor_memo_h__

#include "opencsgConfig.h"
#include "area.h"
#include "channelManager.h"
#include <vector>

namespace OpenCSG {

    class ScissorMemo {
    public:
        /// convenience class for handling scissoring. All areas
        /// are given in normal device coordinates [-1, 1] x [-1, 1].
        ScissorMemo();

        /// for each channel, the currently set area can be stored
        void store(Channel);
        /// restores the area for the channel
        void recall(Channel);

        /// enables scissoring of area
        void enableScissor() const;
        /// disables scissoring
        void disableScissor() const;

        /// checks if the depth bounds test is enabled
        bool isDepthBoundsTestEnabled() const;
        /// enables depth bounds test of volume (if supported by graphics hardware)
        void enableDepthBounds() const;
        /// enables depth bounds test of back of current volume (if supported by graphics hardware)
        void enableDepthBoundsBack() const;
        /// enables depth bounds test of front of current volume (if supported by graphics hardware)
        void enableDepthBoundsFront() const;
        /// disables depth bounds test of volume (if supported by graphics hardware)
        void disableDepthBounds() const;

        /// sets the maximum area of the intersected CSG-primitives
        /// by intersecting the bounding boxes of them.
        void setIntersected(const std::vector<Primitive*>& primitives);

        /// sets the area of primitives whose visilibity is determined next.
        void setCurrent(const std::vector<Primitive*>& primitives);
        /// returns the maximum area of the intersected CSG-primitives
        const NDCVolume& getIntersectedArea() const;
        /// returns the maximum area of the intersected CSG-primitives
        /// intersected with the currently used primitives
        const NDCVolume& getCurrentArea() const;

    protected:
        /// calculates rendering area by intersecting intersected
        /// and current. This is invoked implicetely.
        void calculateArea();

    private:
        ScissorMemo(const ScissorMemo&);
        ScissorMemo& operator=(const ScissorMemo&);

        NDCVolume mIntersection;
        NDCVolume mCurrent;
        NDCVolume mArea;
        std::vector<NDCVolume> mScissor;

        bool mUseDepthBoundsTest;
    };

} // namespace OpenCSG

#endif // __OpenCSG__scissor_memo_h__
