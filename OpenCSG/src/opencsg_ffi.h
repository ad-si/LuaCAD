// C FFI wrapper for OpenCSG
// Exposes the C++ OpenCSG API through a C ABI for use from Rust.

#ifndef OPENCSG_FFI_H
#define OPENCSG_FFI_H

#ifdef __cplusplus
extern "C" {
#endif

// Callback type for rendering a primitive's geometry.
// Called by OpenCSG during its rendering passes.
typedef void (*opencsg_render_fn)(void *user_data);

// Opaque handle to an OpenCSG primitive.
typedef struct opencsg_primitive_t opencsg_primitive_t;

// Operation constants (matching OpenCSG::Operation)
#define OPENCSG_INTERSECTION 0
#define OPENCSG_SUBTRACTION 1

// Algorithm constants (matching OpenCSG::Algorithm)
#define OPENCSG_ALGORITHM_AUTOMATIC 0
#define OPENCSG_ALGORITHM_GOLDFEATHER 1
#define OPENCSG_ALGORITHM_SCS 2

// OptionType constants (matching OpenCSG::OptionType)
#define OPENCSG_OPTION_ALGORITHM 0
#define OPENCSG_OPTION_DEPTH_COMPLEXITY 1
#define OPENCSG_OPTION_OFFSCREEN 2

// Initialize OpenCSG's internal GLAD GL loader.
// Must be called once after the GL context is made current.
void opencsg_init_gl(void);

// Create a new primitive with a render callback.
// operation: OPENCSG_INTERSECTION or OPENCSG_SUBTRACTION
// convexity: max number of front faces at any point (1 for convex shapes)
// callback: function that draws the primitive's geometry using GL calls
// user_data: passed to callback on each invocation
opencsg_primitive_t *opencsg_primitive_new(
    int operation,
    unsigned int convexity,
    opencsg_render_fn callback,
    void *user_data);

// Free a primitive.
void opencsg_primitive_free(opencsg_primitive_t *prim);

// Set the bounding box of a primitive in normalized device coordinates.
void opencsg_primitive_set_bounding_box(
    opencsg_primitive_t *prim,
    float minx, float miny, float minz,
    float maxx, float maxy, float maxz);

// Perform CSG rendering on an array of primitives.
// This sets the depth buffer to the CSG result. After this call,
// re-render the primitives with glDepthFunc(GL_EQUAL) to shade them.
void opencsg_render_primitives(
    opencsg_primitive_t **primitives,
    int count);

// Set an OpenCSG option (algorithm, depth complexity strategy, etc.)
void opencsg_set_option(int option_type, int setting);

// Release OpenGL resources allocated by OpenCSG for the current context.
void opencsg_free_resources(void);

#ifdef __cplusplus
}
#endif

#endif // OPENCSG_FFI_H
