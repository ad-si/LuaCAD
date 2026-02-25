// OpenCSG - library for image-based CSG rendering for OpenGL
// Copyright (C) 2025-2026, Florian Kirsch
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
// sequencer.h
//
// algorithms to create short permutation embedding sequences.
// c.f. https://oeis.org/A062714. Such sequences are used in the
// SCS algorithm.
//

#ifndef __OpenCSG__sequencer_h__
#define __OpenCSG__sequencer_h__

#include "opencsgConfig.h"

#include <algorithm>

namespace OpenCSG {

    class Sequencer
    {
    public:
        virtual ~Sequencer()
        {
        }

        virtual size_t index(size_t position) const = 0;
        virtual size_t size() const = 0;
        virtual size_t sizeForDepthComplexity(size_t dc) const = 0;

    protected:
        Sequencer(size_t numElems)
            : n(numElems)
        {
        }
        size_t n;
    };

    class SimpleSequencer : public Sequencer
    {
    public:
        SimpleSequencer(size_t numElems)
          : Sequencer(numElems)
        {
        }
        virtual size_t index(size_t position) const
        {
            return position % n;
        }
        virtual size_t size() const
        {
            return n * (n - 1) + 1;
        }
        virtual size_t sizeForDepthComplexity(size_t dc) const
        {
            return n * dc;
        }

    };

    class BouncingSequencer : public Sequencer
    {
    public:
        BouncingSequencer(size_t numElems)
            : Sequencer(numElems)
        {
        }
        virtual size_t index(size_t position) const
        {
            if (n == 1)
                return 0;
            size_t wave = position % (n + n - 2);
            if (wave < n)
                return wave;
            else
                return (n + n - 2) - wave;
        }
        virtual size_t size() const
        {
            return n * (n - 1) + 1;
        }
        virtual size_t sizeForDepthComplexity(size_t dc) const
        {
            return (dc & 1) == 1
                ?  (dc - 1) * (n - 1) + n
                :     dc    * (n - 1) + 1;
        }
    };

    class SchoenfieldSequencer : public Sequencer
    {
    public:
        SchoenfieldSequencer(size_t numElems)
            : Sequencer(numElems)
        {
        }
        virtual size_t index(size_t position) const
        {
            if (n == 1)
                return 0;
            if (n == 2)
                return position & 1;
            if (position < n)
                return position;
            else if ((position - 1) % (n - 1) == 0)
                return 0;
            else
                return (position * (n - 2) / (n - 1)) % (n - 1) + 1;
        }
        virtual size_t size() const
        {
            if (n == 1)
                return 1;
            if (n == 2)
                return 3;
            return n * n - 2 * n + 4;
        }
        virtual size_t sizeForDepthComplexity(size_t dc) const
        {
            return std::min(size(), n * dc);
        }
    };

} // namespace OpenCSG

#endif // __OpenCSG__sequencer_h__
