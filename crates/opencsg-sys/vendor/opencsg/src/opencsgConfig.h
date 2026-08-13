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
// opencsgConfig.h
//
// compiler specific settings for OpenCSG
//

#ifndef __OpenCSG__opencsg_config_h__
#define __OpenCSG__opencsg_config_h__

// WINDOWS
#ifdef WIN32

// blend out some compiler warnings
#ifdef __INTEL_COMPILER // intel compiler

# pragma warning(disable: 985) // identifier ... was truncated in debug information

#elif _MSC_VER // microsoft visual studio compiler without intel compiler
#if _MSC_VER < 1300 // MSVC++ <= 6.0

# pragma warning(disable: 4786) // identifier was truncated to X characters in the debug information

#endif
#endif // __INTEL_COMPILER || _MSC_VER

#ifdef _MSC_VER // microsoft visual studio compiler and intel compiler
#if _MSC_VER < 1300 // MSVC++ <= 6.0
    // MSVC does not have std::min and std::max unless using .NET. (>=7.0) 
    // for VC 6.0, we define those function templates in namespace std.
    // (actually copied from boost).
    // min / max macros in windows.h additionally circumvent a correct
    // use of std::min. To circumvent this macro expansion, in OpenCSG
    // all occurencies of std::min and std::max are put into brackets.
    // Hopefully, this will fix all compilation problems in the future.

    #ifdef min
    #undef min
    #endif

    #ifdef max
    #undef max
    #endif

    namespace std {

        template <class T>
        inline const T& min(const T& a, const T& b) {
           return b < a ? b : a;
        }

        template <class T>
        inline const T& max(const T& a, const T& b) {
           return a < b ? b : a;
        }

        template <class T, class Compare>
        inline const T& min(const T& a, const T& b, Compare comp) {
           return comp(b, a) ? b : a;
        }

        template <class T, class Compare>
        inline const T& max(const T& a, const T& b, Compare comp) {
           return comp(a, b) ? b : a;
        }

        typedef ::size_t size_t;  // MSVC6 lacks std::size_t

    } // namespace std

#endif // _MSC_VER < 1300, i.e., VS <= 6.0
#endif // _MSC_VER

#endif // WIN32

#endif // __OpenCSG__opencsg_config_h__
