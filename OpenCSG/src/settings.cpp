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
// settings.cpp
//

#include "opencsgConfig.h"
#include <opencsg.h>

#include <algorithm>
#include <list>
#include <string>
#include <utility>

namespace OpenCSG {

    // Stores constant strings of all vertex shaders that were ever provided to OpenCSG.
    // This would compile with std::set as well. But I expect very few shaders here, only.
    // Note that std::vector would be incorrect, because when resizing the vector,
    // the strings could be relocated. This must not happen, because the string pointers
    // are used as keys for the actual GLSL program IDs.
    typedef std::list<std::string> VertexShaders;

    static VertexShaders gVertexShaders;
    static VertexShaders::iterator gCurrentVertexShader = gVertexShaders.end();

    void setVertexShader(const std::string& vertexShader)
    {
        if (vertexShader.length() == 0)
        {
            gCurrentVertexShader = gVertexShaders.end();
            return;
        }

        if (gCurrentVertexShader != gVertexShaders.end() && *gCurrentVertexShader == vertexShader)
            return;

        VertexShaders::iterator it = std::find(gVertexShaders.begin(), gVertexShaders.end(), vertexShader);
        if (it != gVertexShaders.end())
        {
            gCurrentVertexShader = it;
            return;
        }

        gCurrentVertexShader = gVertexShaders.insert(it, vertexShader);
    }

    const char* getVertexShader()
    {
        if (gCurrentVertexShader == gVertexShaders.end())
            return 0;

        return gCurrentVertexShader->c_str();
    }

    static int* gSetting = 0;

    static void initIntOptions() {
        if (!gSetting) {
            gSetting = new int[OptionTypeUnused];
            for (int i=0; i<OptionTypeUnused; ++i) {
                gSetting[i] = 0;
            }
        }
    }

    void setOption(OptionType option, int newSetting) {
        if ((unsigned int)option < OptionTypeUnused) {
            initIntOptions();
            gSetting[option] = newSetting;
        }
    }

    int getOption(OptionType option) {
        initIntOptions();
        if ((unsigned int)option < OptionTypeUnused) {
            return gSetting[option];
        }

        return 0;
    }

} // namespace OpenCSG
