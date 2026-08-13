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
// primitive.cpp
//
// implements Primitive interface (declared in opencsg.h)
//

#include "opencsgConfig.h"
#include <opencsg.h>
#include <cassert>

namespace OpenCSG {
    
    Primitive::Primitive(Operation o, unsigned int c) :
        mOperation(o),
        mConvexity(c),
        mMinx(-1.0f),
        mMiny(-1.0f),
        mMinz(-1.0f),
        mMaxx(1.0f),
        mMaxy(1.0f),
        mMaxz(1.0f) {
    }

    Primitive::~Primitive() { }

    void Primitive::setOperation(Operation o) {
        mOperation = o;
    }

    Operation Primitive::getOperation() const {
        return mOperation;
    }

    void Primitive::setConvexity(unsigned int c) {
        mConvexity = c;
    }

    unsigned int Primitive::getConvexity() const {
        return mConvexity;
    }

    void Primitive::setBoundingBox(float minx, float miny, float minz,
                                   float maxx, float maxy, float maxz) {
        assert(minx <= maxx);
        assert(miny <= maxy);
        assert(minz <= maxz);
        mMinx = minx;
        mMiny = miny;
        mMinz = minz;
        mMaxx = maxx;
        mMaxy = maxy;
        mMaxz = maxz;
    }


    void Primitive::getBoundingBox(float& minx, float& miny, float& minz,
                                   float& maxx, float& maxy, float& maxz) const {
        minx = mMinx;
        miny = mMiny;
        minz = mMinz;
        maxx = mMaxx;
        maxy = mMaxy;
        maxz = mMaxz;
    }

} // namespace OpenCSG
