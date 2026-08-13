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
// batch.h
//
// object space optimization: If possible, several primitives are treated 
// as a single batch
//

#ifndef __OpenCSG__batch_h__
#define __OpenCSG__batch_h__

#include "opencsgConfig.h"
#include <vector>
#include <cstddef>

namespace OpenCSG {

    class Primitive;

    /// a Batch is a list of primitives that do not overlap in screen space
    typedef std::vector<Primitive*> Batch;

    class Batcher {
    public:
        /// subdivides an array of primitives into batches
        Batcher(const std::vector<Primitive*>& primitives);

        /// returns all batches as constant reference
        const std::vector<Batch>& batches() const;
        /// returns first batch
        std::vector<Batch>::const_iterator begin() const;
        /// returns end of batches
        std::vector<Batch>::const_iterator end() const;
        /// return number of batches
        std::size_t size() const;

    private:
        std::vector<Batch> mBatches;
    };

} // namespace OpenCSG

#endif // __OpenCSG__batch_h__
