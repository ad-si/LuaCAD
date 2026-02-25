// OpenCSG - library for image-based CSG rendering for OpenGL
// Copyright (C) 2004-2026, Florian Kirsch,
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
// occlusionQuery.cpp
//

#include "opencsgConfig.h"
#include "openglExt.h"
#include "occlusionQuery.h"

namespace OpenCSG {

    namespace OpenGL {

        class OcclusionQueryARB : public OcclusionQuery {
        public:
            OcclusionQueryARB();
            virtual ~OcclusionQueryARB();

            virtual void beginQuery();
            virtual void endQuery();
            virtual unsigned int getQueryResult();

        private:
            friend OcclusionQuery* getOcclusionQuery(bool exactNumberNeeded);
            GLenum mQueryType;
            GLuint mQueryObject;
        };

        OcclusionQueryARB::OcclusionQueryARB() : mQueryType(GL_SAMPLES_PASSED_ARB) {
            glGenQueriesARB(1, &mQueryObject);
        }

        OcclusionQueryARB::~OcclusionQueryARB() {
            glDeleteQueriesARB(1, &mQueryObject);
        }

        void OcclusionQueryARB::beginQuery() {
            glBeginQueryARB(mQueryType, mQueryObject);
        }

        void OcclusionQueryARB::endQuery() {
            glEndQueryARB(mQueryType);
        }

        unsigned int OcclusionQueryARB::getQueryResult() {
            GLuint fragmentCount;
            glGetQueryObjectuivARB(mQueryObject, GL_QUERY_RESULT_ARB, &fragmentCount);
            return fragmentCount;
        }



        class OcclusionQueryNV : public OcclusionQuery {
        public:
            OcclusionQueryNV();
            virtual ~OcclusionQueryNV();

            virtual void beginQuery();
            virtual void endQuery();
            virtual unsigned int getQueryResult();

        private:
            GLuint mQueryObject;
        };

        OcclusionQueryNV::OcclusionQueryNV() {
            glGenOcclusionQueriesNV(1, &mQueryObject);
        }

        OcclusionQueryNV::~OcclusionQueryNV() {
            glDeleteOcclusionQueriesNV(1, &mQueryObject);
        }

        void OcclusionQueryNV::beginQuery() {
            glBeginOcclusionQueryNV(mQueryObject);
        }

        void OcclusionQueryNV::endQuery() {
            glEndOcclusionQueryNV();
        }

        unsigned int OcclusionQueryNV::getQueryResult() {
            GLuint fragmentCount;
            glGetOcclusionQueryuivNV(mQueryObject, GL_PIXEL_COUNT_NV, &fragmentCount);
            return fragmentCount;
        }



        OcclusionQuery* getOcclusionQuery(bool exactNumberNeeded) {

            if (!exactNumberNeeded && OPENCSG_HAS_EXT(ARB_occlusion_query2)) {
                OcclusionQueryARB* occlusionQuery = new OcclusionQueryARB;
                occlusionQuery->mQueryType = GL_ANY_SAMPLES_PASSED;
                return occlusionQuery;
            }

            if (OPENCSG_HAS_EXT(ARB_occlusion_query)) {
                OcclusionQueryARB* occlusionQuery = new OcclusionQueryARB;
                occlusionQuery->mQueryType = GL_SAMPLES_PASSED_ARB;
                return occlusionQuery;
            }

            if (OPENCSG_HAS_EXT(NV_occlusion_query)) {
                return new OcclusionQueryNV;
            }

            return 0;
        }

    } // namespace OpenGL

} // namespace OpenCSG
