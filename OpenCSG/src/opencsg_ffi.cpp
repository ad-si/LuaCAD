// C FFI wrapper for OpenCSG
// Implements the C ABI functions declared in opencsg_ffi.h.

#include <opencsg.h>
#include <vector>
#include "opencsg_ffi.h"
#include "openglExt.h"

// A Primitive subclass that delegates render() to a C function pointer.
class CallbackPrimitive : public OpenCSG::Primitive {
public:
  CallbackPrimitive(OpenCSG::Operation op, unsigned int convexity,
                    opencsg_render_fn callback, void *user_data)
      : Primitive(op, convexity), mCallback(callback), mUserData(user_data) {}

  void render() override {
    if (mCallback) {
      mCallback(mUserData);
    }
  }

private:
  opencsg_render_fn mCallback;
  void *mUserData;
};

extern "C" {

void opencsg_init_gl(void) { OpenCSG::initExtensionLibrary(); }

int opencsg_glad_version(void) {
  return OpenCSG::GLAD_GL_VERSION_2_0 ? 20 :
         OpenCSG::GLAD_GL_VERSION_1_5 ? 15 :
         OpenCSG::GLAD_GL_VERSION_1_4 ? 14 :
         OpenCSG::GLAD_GL_VERSION_1_3 ? 13 :
         OpenCSG::GLAD_GL_VERSION_1_2 ? 12 :
         OpenCSG::GLAD_GL_VERSION_1_1 ? 11 :
         OpenCSG::GLAD_GL_VERSION_1_0 ? 10 : 0;
}

void opencsg_glad_fbo_support(int *out_arb_fbo, int *out_ext_fbo, int *out_ext_pds) {
  *out_arb_fbo = OpenCSG::GLAD_GL_ARB_framebuffer_object;
  *out_ext_fbo = OpenCSG::GLAD_GL_EXT_framebuffer_object;
  *out_ext_pds = OpenCSG::GLAD_GL_EXT_packed_depth_stencil;
}

opencsg_primitive_t *opencsg_primitive_new(int operation,
                                           unsigned int convexity,
                                           opencsg_render_fn callback,
                                           void *user_data) {
  auto op = (operation == OPENCSG_SUBTRACTION) ? OpenCSG::Subtraction
                                               : OpenCSG::Intersection;
  auto *prim = new CallbackPrimitive(op, convexity, callback, user_data);
  return reinterpret_cast<opencsg_primitive_t *>(prim);
}

void opencsg_primitive_free(opencsg_primitive_t *prim) {
  delete reinterpret_cast<CallbackPrimitive *>(prim);
}

void opencsg_primitive_set_bounding_box(opencsg_primitive_t *prim, float minx,
                                        float miny, float minz, float maxx,
                                        float maxy, float maxz) {
  if (prim) {
    reinterpret_cast<OpenCSG::Primitive *>(prim)->setBoundingBox(minx, miny,
                                                                 minz, maxx,
                                                                 maxy, maxz);
  }
}

void opencsg_render_primitives(opencsg_primitive_t **primitives, int count) {
  std::vector<OpenCSG::Primitive *> prims;
  prims.reserve(count);
  for (int i = 0; i < count; i++) {
    prims.push_back(reinterpret_cast<OpenCSG::Primitive *>(primitives[i]));
  }
  OpenCSG::render(prims);
}

void opencsg_set_option(int option_type, int setting) {
  OpenCSG::setOption(static_cast<OpenCSG::OptionType>(option_type), setting);
}

void opencsg_free_resources(void) { OpenCSG::freeResources(); }

} // extern "C"
