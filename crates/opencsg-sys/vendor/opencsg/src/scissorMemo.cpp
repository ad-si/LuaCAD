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
// scissorMemo.cpp
//

#include "opencsgConfig.h"
#include <opencsg.h>
#include "openglExt.h"
#include "openglHelper.h"
#include "scissorMemo.h"

#include <algorithm>

namespace OpenCSG {

    ScissorMemo::ScissorMemo() : 
        mIntersection(NDCVolume(-1.0f, -1.0f, 0.0f, 1.0f, 1.0f, 1.0f)),
        mCurrent(NDCVolume(1.0f, 1.0f, 1.0f, -1.0f, -1.0f, 0.0f)),
        mArea(NDCVolume(-1.0f, -1.0f, 0.0f, 1.0f, 1.0f, 1.0f)),
        mScissor(std::vector<NDCVolume>(AllChannels + 1)),
        mUseDepthBoundsTest(false)
    {
        int optimizationSetting = getOption(DepthBoundsOptimization);
        if (optimizationSetting == OptimizationForceOn)
            mUseDepthBoundsTest = true;
        else if (   optimizationSetting == OptimizationDefault
                 || optimizationSetting == OptimizationOff)
            mUseDepthBoundsTest = false;
        else if (optimizationSetting == OptimizationOn)
            mUseDepthBoundsTest = (OPENCSG_HAS_EXT(EXT_depth_bounds_test) != 0);
    }

    void ScissorMemo::store(Channel ch) {
        mScissor[ch] = mArea;
    }

    void ScissorMemo::recall(Channel ch) {
        mArea = mScissor[ch];
    }

    void ScissorMemo::enableScissor() const {
        OpenGL::scissor(mArea);
    }

    void ScissorMemo::disableScissor() const {
        glDisable(GL_SCISSOR_TEST);
    }

    bool ScissorMemo::isDepthBoundsTestEnabled() const
    {
        return mUseDepthBoundsTest;
    }

    void ScissorMemo::enableDepthBounds() const {
        if (!mUseDepthBoundsTest)
            return;
        glDepthBoundsEXT(mArea.minz, mArea.maxz);
        glEnable(GL_DEPTH_BOUNDS_TEST_EXT);
    }

    void ScissorMemo::enableDepthBoundsBack() const {
        if (!mUseDepthBoundsTest)
            return;
        glDepthBoundsEXT(0.0f, mCurrent.maxz);
        glEnable(GL_DEPTH_BOUNDS_TEST_EXT);
    }

    void ScissorMemo::enableDepthBoundsFront() const {
        if (!mUseDepthBoundsTest)
            return;
        glDepthBoundsEXT(mCurrent.minz, 1.0);
        glEnable(GL_DEPTH_BOUNDS_TEST_EXT);
    }

    void ScissorMemo::disableDepthBounds() const {
        if (!mUseDepthBoundsTest)
            return;
        glDisable(GL_DEPTH_BOUNDS_TEST_EXT);
    }

    void ScissorMemo::setIntersected(const std::vector<Primitive*>& primitives) {

        float& minx = mIntersection.minx;
        float& miny = mIntersection.miny;
        float& minz = mIntersection.minz;
        float& maxx = mIntersection.maxx;
        float& maxy = mIntersection.maxy;
        float& maxz = mIntersection.maxz;

        // dont let intersected area exceed scissor region set by the application (outside OpenCSG)
        const int dx = OpenGL::canvasPos[2] - OpenGL::canvasPos[0];
        const int dy = OpenGL::canvasPos[3] - OpenGL::canvasPos[1];

        const float sx  = 2.0f * (static_cast<float>(OpenGL::scissorPos[0]) / static_cast<float>(dx)) - 1.0f;
        const float sy  = 2.0f * (static_cast<float>(OpenGL::scissorPos[1]) / static_cast<float>(dy)) - 1.0f;
        const float swx = 2.0f * (static_cast<float>(OpenGL::scissorPos[2] + OpenGL::scissorPos[0]) / static_cast<float>(dx)) - 1.0f;
        const float swy = 2.0f * (static_cast<float>(OpenGL::scissorPos[3] + OpenGL::scissorPos[1]) / static_cast<float>(dy)) - 1.0f;

        minx = sx; maxx = swx; 
        miny = sy; maxy = swy;

        minz = 0.0f; maxz = 1.0f; // might read current depth value, but usefulness is unclear 

        for (std::vector<Primitive*>::const_iterator itr = primitives.begin(); itr != primitives.end(); ++itr) {
            if ((*itr)->getOperation() == Intersection) {
                float tminx, tminy, tminz, tmaxx, tmaxy, tmaxz;
                (*itr)->getBoundingBox(tminx, tminy, tminz, tmaxx, tmaxy, tmaxz);

                minx = (std::max)(minx, tminx);
                miny = (std::max)(miny, tminy);
                minz = (std::max)(minz, tminz);
                maxx = (std::min)(maxx, tmaxx);
                maxy = (std::min)(maxy, tmaxy);
                maxz = (std::min)(maxz, tmaxz);
            }
        }

        minx = (std::max)(-1.0f, minx);
        minx = (std::min)( 1.0f, minx);
        miny = (std::max)(-1.0f, miny);
        miny = (std::min)( 1.0f, miny);
        minz = (std::max)( 0.0f, minz);
        minz = (std::min)( 1.0f, minz);
        maxx = (std::max)(-1.0f, maxx);
        maxx = (std::min)( 1.0f, maxx);
        maxy = (std::max)(-1.0f, maxy);
        maxy = (std::min)( 1.0f, maxy);
        maxz = (std::max)( 0.0f, maxz);
        maxz = (std::min)( 1.0f, maxz);

        calculateArea();
    }

    const NDCVolume& ScissorMemo::getIntersectedArea() const {
        return mIntersection;
    }

    const NDCVolume& ScissorMemo::getCurrentArea() const {
        return mArea;
    }

    void ScissorMemo::setCurrent(const std::vector<Primitive*>& primitives) {

        float& minx = mCurrent.minx;
        float& miny = mCurrent.miny;
        float& minz = mCurrent.minz;
        float& maxx = mCurrent.maxx;
        float& maxy = mCurrent.maxy;
        float& maxz = mCurrent.maxz;

        minx = 1.0f;  maxx = -1.0f;
        miny = 1.0f;  maxy = -1.0f;
        minz = 1.0f;  maxz =  0.0f;

        for (std::vector<Primitive*>::const_iterator itr = primitives.begin(); itr != primitives.end(); ++itr) {
            float tminx, tminy, tminz, tmaxx, tmaxy, tmaxz;
            (*itr)->getBoundingBox(tminx, tminy, tminz, tmaxx, tmaxy, tmaxz);

            minx = (std::min)(minx, tminx);
            miny = (std::min)(miny, tminy);
            minz = (std::min)(minz, tminz);
            maxx = (std::max)(maxx, tmaxx);
            maxy = (std::max)(maxy, tmaxy);
            maxz = (std::max)(maxz, tmaxz);
        }

        minx = (std::max)(-1.0f, minx);
        minx = (std::min)( 1.0f, minx); 
        miny = (std::max)(-1.0f, miny);
        miny = (std::min)( 1.0f, miny);
        minz = (std::max)( 0.0f, minz);
        minz = (std::min)( 1.0f, minz);
        maxx = (std::max)(-1.0f, maxx);
        maxx = (std::min)( 1.0f, maxx);
        maxy = (std::max)(-1.0f, maxy);
        maxy = (std::min)( 1.0f, maxy);
        maxz = (std::max)( 0.0f, maxz);
        maxz = (std::min)( 1.0f, maxz);

        calculateArea();
    }

    void ScissorMemo::calculateArea() {
        mArea.minx = (std::max)(mCurrent.minx, mIntersection.minx);
        mArea.miny = (std::max)(mCurrent.miny, mIntersection.miny);
        mArea.minz = (std::max)(mCurrent.minz, mIntersection.minz);
        mArea.maxx = (std::min)(mCurrent.maxx, mIntersection.maxx);
        mArea.maxy = (std::min)(mCurrent.maxy, mIntersection.maxy);
        mArea.maxz = (std::min)(mCurrent.maxz, mIntersection.maxz);
    }

} // namespace OpenCSG
