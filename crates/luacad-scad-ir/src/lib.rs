//! The CSG intermediate representation: a tree (later a DAG) of geometry
//! operations produced by evaluating a program, and consumed by the geometry
//! kernel.
//!
//! For M0 this is a plain tree. Structural hashing / DAG deduplication and
//! canonicalization (n-ary union flattening, transform folding) arrive in M3/M4.

pub type Vec3 = [f64; 3];
pub type Vec2 = [f64; 2];

/// The `$fn` / `$fa` / `$fs` values in effect when a curved primitive was
/// instantiated. The concrete fragment count is derived from these plus the
/// radius by the geometry kernel (bit-exact fragment formula).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FragmentSpec {
    pub fn_: f64,
    pub fa: f64,
    pub fs: f64,
}

impl Default for FragmentSpec {
    fn default() -> Self {
        FragmentSpec {
            fn_: 0.0,
            fa: 12.0,
            fs: 2.0,
        }
    }
}

/// A node in the CSG tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// No geometry.
    Empty,
    /// An implicit group of children (unioned for rendering, kept as a list so
    /// transforms apply to the group as a whole).
    Group(Vec<Node>),

    // --- primitives ---
    Cube {
        size: Vec3,
        center: bool,
    },
    Sphere {
        r: f64,
        frags: FragmentSpec,
    },
    Cylinder {
        h: f64,
        r1: f64,
        r2: f64,
        center: bool,
        frags: FragmentSpec,
    },
    /// A user-specified mesh: explicit points and (possibly polygonal) faces.
    Polyhedron {
        points: Vec<Vec3>,
        faces: Vec<Vec<u32>>,
    },

    // --- 2D primitives (produce a set of contours, not a mesh) ---
    Square {
        size: Vec2,
        center: bool,
    },
    Circle {
        r: f64,
        frags: FragmentSpec,
    },
    Polygon {
        points: Vec<Vec2>,
        /// Optional contour index lists; when absent, all points form one path.
        paths: Option<Vec<Vec<u32>>>,
    },

    // --- 2D -> 3D operations ---
    LinearExtrude {
        height: f64,
        center: bool,
        /// Total twist in degrees over the height.
        twist: f64,
        /// Scale of the top relative to the bottom.
        scale: Vec2,
        /// Number of intermediate layers (>=1).
        slices: u32,
        child: Box<Node>,
    },
    RotateExtrude {
        /// Sweep angle in degrees (360 = full revolution).
        angle: f64,
        frags: FragmentSpec,
        child: Box<Node>,
    },
    /// 2D offset. `r` (rounded) or `delta` (mitred/chamfered) grows (>0) or
    /// shrinks (<0) the child's contours.
    Offset {
        r: f64,
        delta: f64,
        chamfer: bool,
        frags: FragmentSpec,
        child: Box<Node>,
    },

    // --- transforms ---
    Translate {
        v: Vec3,
        child: Box<Node>,
    },
    /// Euler angles in degrees, applied X then Y then Z (OpenSCAD convention).
    Rotate {
        deg: Vec3,
        child: Box<Node>,
    },
    Scale {
        v: Vec3,
        child: Box<Node>,
    },
    /// Reflect across the plane through the origin with normal `v`.
    Mirror {
        v: Vec3,
        child: Box<Node>,
    },
    /// Apply a 4x4 affine matrix (row-major).
    MultMatrix {
        m: [[f64; 4]; 4],
        child: Box<Node>,
    },
    /// Scale the child so its bounding box matches `new` (0 = keep, unless the
    /// matching `auto` flag scales it proportionally).
    Resize {
        new: Vec3,
        auto: [bool; 3],
        child: Box<Node>,
    },

    // --- booleans ---
    Union(Vec<Node>),
    Difference(Vec<Node>),
    Intersection(Vec<Node>),
    /// Convex hull of all children.
    Hull(Vec<Node>),
    /// Minkowski sum of all children.
    Minkowski(Vec<Node>),
    /// An imported mesh file (raw bytes + lowercase format, e.g. "stl").
    Import {
        data: Vec<u8>,
        format: String,
    },
    /// `projection(cut)` — flatten a 3D child to 2D. `cut=true` sections at z=0.
    Projection {
        cut: bool,
        child: Box<Node>,
    },

    // --- display attributes (preview only; transparent to fused geometry) ---
    /// `color(c, alpha)` — tints the child's *result* in the preview. `rgba` is
    /// linear 0..1. Geometry is unaffected (the child renders identically fused).
    Color {
        rgba: [f32; 4],
        child: Box<Node>,
    },
    /// `#` highlight — child is drawn translucent-red in the preview but is
    /// otherwise normal geometry (kept in exports).
    Highlight(Box<Node>),
    /// `%` background — child is drawn translucent-gray in the preview and is
    /// **excluded** from the rendered/exported mesh (documented OpenSCAD `%`).
    Background(Box<Node>),

    // --- provenance (preview only; transparent to fused geometry) ---
    /// Source provenance: tags the child's geometry with a source byte-span for
    /// editor↔preview linking. Transparent to the fused geometry, the structural
    /// hash, and all mesh I/O (like [`Node::Color`]); only the separate
    /// provenance partition pass reads the span, emitting a per-statement pickable
    /// group. Wrapped around each `ModuleCall` result during evaluation.
    Provenance {
        span: core::ops::Range<usize>,
        child: Box<Node>,
    },
}

impl Node {
    /// Build a node from a list of children, wrapping in a group as needed.
    /// An empty list becomes [`Node::Empty`]; a single child is returned as-is.
    pub fn group(mut children: Vec<Node>) -> Node {
        children.retain(|c| !matches!(c, Node::Empty));
        match children.len() {
            0 => Node::Empty,
            1 => children.pop().unwrap(),
            _ => Node::Group(children),
        }
    }
}
