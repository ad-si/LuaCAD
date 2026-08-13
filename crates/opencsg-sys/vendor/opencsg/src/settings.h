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
// settings.h
//
// global OpenCSG settings
//

#ifndef __OpenCSG__settings_h__
#define __OpenCSG__settings_h__

#include "opencsgConfig.h"
#include <opencsg.h>

#include <string>

namespace OpenCSG {

    /// redeclared from opencsg.h
    void setVertexShader(const std::string& vertexShader);
    const char* getVertexShader();

    /// redeclared from opencsg.h
    void setOption(OptionType option, int newSetting);
    /// redeclared from opencsg.h
    int  getOption(OptionType option);

} // namespace OpenCSG

#endif // __OpenCSG__settings_h__

