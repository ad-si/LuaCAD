// OpenCSG - library for image-based CSG rendering for OpenGL
// Copyright (C) 2002-2026, Florian Kirsch
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
// batch.cpp
//

#include "opencsgConfig.h"
#include <opencsg.h>
#include "batch.h"
#include "primitiveHelper.h"

namespace OpenCSG {

    class FullscreenPrimitive : public Primitive
    {
    public:
        FullscreenPrimitive()
            : Primitive(OpenCSG::Intersection, 1)
        {
        }

        virtual void render()
        {
        }
    };

    Batcher::Batcher(const std::vector<Primitive*>& primitives) { 

        FullscreenPrimitive fullscreen;

        const std::size_t numberOfPrimitives = primitives.size();
        mBatches.reserve(numberOfPrimitives);

        std::vector<Batch> batchCandidates;
        batchCandidates.reserve(numberOfPrimitives);

        for (std::vector<Primitive*>::const_iterator itr = primitives.begin(); itr != primitives.end(); ++itr) {

            // primitive completely outside viewport, no need to process it any further
            if (!Algo::intersectXY(*itr, &fullscreen))
                continue;

            // fullscreen is completely part of the primitive's bounding box,
            // no other primitive can be part of the same batch
            if (Algo::containsXY(&fullscreen, *itr)) {
                mBatches.push_back(Batch());
                Batch& batch = mBatches.back();
                batch.push_back(*itr);
            }
            else
            {
                std::vector<Batch>::iterator batchItr = batchCandidates.begin();
                std::vector<Batch>::iterator batchEnd = batchCandidates.end();

                for ( ; batchItr != batchEnd; ++batchItr) {

                    Batch & batch = *batchItr;
                    Batch::const_iterator batchPrimitives = batch.begin();
                    Batch::const_iterator batchPrimitivesEnd = batch.end();

                    // if the primitive does not intersect any of the primitives in
                    // the current batch, we can add the primitive to that batch
                    for ( ; batchPrimitives != batchPrimitivesEnd; ++batchPrimitives) {
                        if (Algo::intersectXY(*itr, *batchPrimitives)) {
                            break;
                        }
                    }
                    if (batchPrimitives == batchPrimitivesEnd) {
                        batch.push_back(*itr);
                        break;
                    }
                }

                // primitive could not added to any batch -> create a new batch
                if (batchItr == batchEnd) {
                    batchCandidates.push_back(Batch());
                    Batch& batch = batchCandidates.back();
                    batch.push_back(*itr);
                }
            }
        }

        // Could do here the following. But since since batchCandidates is not needed anymore,
        // we can use the std::vector swap trick to avoid calling of the copy constructor.
        // std::copy(batchCandidates.begin(), batchCandidates.end(), std::back_inserter(mBatches));

        std::vector<Batch>::iterator batchItr = batchCandidates.begin();
        std::vector<Batch>::iterator batchEnd = batchCandidates.end();

        for ( ; batchItr != batchEnd; ++batchItr) {
            mBatches.push_back(Batch());
            Batch& batch = mBatches.back();
            batch.swap(*batchItr);
        }
    }

    const std::vector<Batch>& Batcher::batches() const {
        return mBatches;
    }

    std::vector<Batch>::const_iterator Batcher::begin() const {
        return mBatches.begin();
    }

    std::vector<Batch>::const_iterator Batcher::end() const {
        return mBatches.end();
    }

    std::size_t Batcher::size() const {
        return mBatches.size();
    }

} // namespace OpenCSG

