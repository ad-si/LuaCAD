//! Bounding Volume Hierarchy over a flat primitive array.
//!
//! This replaces the legacy `prime.spatial.KdTree`, whose traversal depended on
//! mutating the ray's length in place to prune farther hits. Here the tree is
//! built with a binned Surface-Area-Heuristic split, stored as a contiguous
//! array of `LinearNode`s, and traversed iteratively with an explicit stack —
//! no recursion and no ray mutation.

use crate::aabb::Aabb;
use crate::geometry::Primitive;
use crate::hit::HitRecord;
use crate::math::Vec3;
use crate::ray::Ray;
use crate::Float;

const MAX_LEAF_PRIMS: usize = 4;
const N_BUCKETS: usize = 12;
/// Relative cost of testing one primitive vs. traversing one interior node.
const TRAVERSAL_COST: Float = 0.125;
/// Relative `t` window within which two hits count as coplanar. Unioned solids
/// of different materials often share exact planes (a part resting on another);
/// inside this window the front-facing hit wins instead of arbitrary traversal
/// order, so buried back-facing faces cannot bleed through.
const TIE_EPS: Float = 1e-5;

#[derive(Clone, Copy)]
struct LinearNode {
    bbox: Aabb,
    /// Leaf (`n_prims > 0`): index of the first primitive.
    /// Interior (`n_prims == 0`): index of the second child (the first child is
    /// always this node's index + 1).
    offset: u32,
    n_prims: u32,
    /// Split axis for interior nodes; used to order near/far traversal.
    axis: u8,
}

/// Per-primitive scratch data used only during construction.
struct BuildInfo {
    prim_index: usize,
    bbox: Aabb,
    centroid: Vec3,
}

#[derive(Clone, Copy)]
struct Bucket {
    count: u32,
    bounds: Aabb,
}

pub struct Bvh {
    nodes: Vec<LinearNode>,
    /// Primitives reordered so each leaf references a contiguous slice.
    prims: Vec<Primitive>,
}

/// A flattened, backend-agnostic view of the BVH and its (reordered) primitives,
/// laid out as plain numeric arrays ready to upload to a GPU. The node and
/// primitive ordering matches the CPU traversal exactly, so a GPU traversal over
/// these buffers produces identical hits.
pub struct FlatBvh {
    /// 6 floats per node: `min.xyz`, `max.xyz`.
    pub node_bounds: Vec<f32>,
    /// 3 u32 per node: `offset`, `n_prims`, `axis`. A leaf has `n_prims > 0` and
    /// `offset` = first primitive index; an interior node has `n_prims == 0`,
    /// `offset` = second-child node index (first child = node index + 1).
    pub node_meta: Vec<u32>,
    /// 1 u32 per primitive: `0` = sphere, `1` = triangle.
    pub prim_kind: Vec<u32>,
    /// 9 floats per primitive. Sphere: `cx, cy, cz, radius, 0, 0, 0, 0, 0`.
    /// Triangle: `v0.xyz, v1.xyz, v2.xyz`.
    pub prim_data: Vec<f32>,
    /// 6 floats per primitive: the 3 per-vertex `(u, v)` texture coordinates of a
    /// triangle. Triangles without UVs use `[0,0, 1,0, 0,1]` so barycentric
    /// interpolation reproduces the bare barycentric coords (matching the CPU
    /// fallback). Unused for spheres (their UVs are analytic).
    pub prim_uv: Vec<f32>,
    /// 1 u32 per primitive: its material index.
    pub prim_material: Vec<u32>,
    pub node_count: usize,
    pub prim_count: usize,
}

impl Default for Bucket {
    fn default() -> Self {
        Bucket {
            count: 0,
            bounds: Aabb::EMPTY,
        }
    }
}

impl Bvh {
    pub fn build(prims: Vec<Primitive>) -> Bvh {
        if prims.is_empty() {
            return Bvh {
                nodes: Vec::new(),
                prims,
            };
        }

        let mut info: Vec<BuildInfo> = prims
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let bbox = p.aabb();
                BuildInfo {
                    prim_index: i,
                    bbox,
                    centroid: bbox.centroid(),
                }
            })
            .collect();

        let mut nodes = Vec::with_capacity(2 * prims.len());
        let mut ordered = Vec::with_capacity(prims.len());
        build_recursive(&prims, &mut info, &mut nodes, &mut ordered);

        Bvh {
            nodes,
            prims: ordered,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.prims.len()
    }

    /// World-space bounds of the whole scene (root node bounds).
    pub fn bounds(&self) -> Aabb {
        self.nodes.first().map_or(Aabb::EMPTY, |n| n.bbox)
    }

    /// The primitive at `idx` in traversal order — the index a [`HitRecord`]
    /// records in its `prim` field.
    #[inline]
    pub fn primitive(&self, idx: usize) -> Option<&Primitive> {
        self.prims.get(idx)
    }

    /// Flatten the tree and its primitives into upload-ready numeric arrays
    /// (see [`FlatBvh`]). Used by the GPU backend; the layout mirrors the CPU
    /// traversal so results match.
    pub fn flatten(&self) -> FlatBvh {
        let mut node_bounds = Vec::with_capacity(self.nodes.len() * 6);
        let mut node_meta = Vec::with_capacity(self.nodes.len() * 3);
        for n in &self.nodes {
            let (lo, hi) = (n.bbox.min, n.bbox.max);
            node_bounds.extend_from_slice(&[lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]);
            node_meta.extend_from_slice(&[n.offset, n.n_prims, n.axis as u32]);
        }

        let mut prim_kind = Vec::with_capacity(self.prims.len());
        let mut prim_data = Vec::with_capacity(self.prims.len() * 9);
        let mut prim_uv = Vec::with_capacity(self.prims.len() * 6);
        let mut prim_material = Vec::with_capacity(self.prims.len());
        for p in &self.prims {
            match p {
                Primitive::Sphere(s) => {
                    prim_kind.push(0);
                    prim_data.extend_from_slice(&[
                        s.center.x, s.center.y, s.center.z, s.radius, 0.0, 0.0, 0.0, 0.0, 0.0,
                    ]);
                    prim_uv.extend_from_slice(&[0.0; 6]);
                    prim_material.push(s.material as u32);
                }
                Primitive::Triangle(t) => {
                    prim_kind.push(1);
                    prim_data.extend_from_slice(&[
                        t.v0.x, t.v0.y, t.v0.z, t.v1.x, t.v1.y, t.v1.z, t.v2.x, t.v2.y, t.v2.z,
                    ]);
                    match t.uvs {
                        Some([a, b, c]) => {
                            prim_uv.extend_from_slice(&[a[0], a[1], b[0], b[1], c[0], c[1]])
                        }
                        // Sentinel: interpolating these reproduces the barycentric
                        // (u, v), matching the CPU's no-UV fallback.
                        None => prim_uv.extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
                    }
                    prim_material.push(t.material as u32);
                }
            }
        }

        FlatBvh {
            node_bounds,
            node_meta,
            prim_kind,
            prim_data,
            prim_uv,
            prim_material,
            node_count: self.nodes.len(),
            prim_count: self.prims.len(),
        }
    }

    /// Closest intersection within `[t_min, t_max]`, or `None`.
    pub fn hit(&self, ray: &Ray, t_min: Float, t_max: Float) -> Option<HitRecord> {
        if self.nodes.is_empty() {
            return None;
        }

        let dir_neg = [ray.dir.x < 0.0, ray.dir.y < 0.0, ray.dir.z < 0.0];
        let mut closest = t_max;
        let mut result: Option<HitRecord> = None;

        // Explicit traversal stack. Depth is O(log n); 64 is ample.
        let mut stack = [0u32; 64];
        let mut sp = 0usize;
        let mut node_idx = 0usize;

        loop {
            let node = &self.nodes[node_idx];
            // Prune with the same slack the tie-break uses, so a node holding
            // a coplanar front-facing triangle is not skipped.
            if node.bbox.hit(ray, t_min, closest * (1.0 + TIE_EPS)) {
                if node.n_prims > 0 {
                    // Leaf: test its primitives. The search window extends a
                    // hair past the current best so exactly coplanar triangles
                    // still reach the tie-break below.
                    let start = node.offset as usize;
                    let prims = &self.prims[start..start + node.n_prims as usize];
                    for (i, p) in prims.iter().enumerate() {
                        // Only widen the window past a hit we already have;
                        // the caller's `t_max` itself stays a hard limit.
                        let window = if result.is_some() {
                            closest * (1.0 + TIE_EPS)
                        } else {
                            closest
                        };
                        if let Some(mut h) = p.hit(ray, t_min, window) {
                            let better = match &result {
                                None => true,
                                // Clearly closer wins; within the coplanar
                                // window a front-facing hit beats a buried
                                // back-facing one.
                                Some(best) => {
                                    h.t * (1.0 + TIE_EPS) < closest
                                        || (h.front_face && !best.front_face)
                                }
                            };
                            if better {
                                h.prim = (start + i) as u32;
                                closest = closest.min(h.t);
                                result = Some(h);
                            }
                        }
                    }
                    if sp == 0 {
                        break;
                    }
                    sp -= 1;
                    node_idx = stack[sp] as usize;
                } else {
                    // Interior: visit the near child first, push the far one.
                    let second_child = node.offset;
                    let first_child = (node_idx + 1) as u32;
                    if dir_neg[node.axis as usize] {
                        stack[sp] = first_child;
                        sp += 1;
                        node_idx = second_child as usize;
                    } else {
                        stack[sp] = second_child;
                        sp += 1;
                        node_idx = first_child as usize;
                    }
                }
            } else {
                if sp == 0 {
                    break;
                }
                sp -= 1;
                node_idx = stack[sp] as usize;
            }
        }

        result
    }

    /// Any-hit occlusion test: returns `true` as soon as *any* primitive is hit
    /// within `[t_min, t_max]`. Much cheaper than [`Bvh::hit`] for shadow rays,
    /// since it stops at the first hit and ignores near/far ordering.
    pub fn occluded(&self, ray: &Ray, t_min: Float, t_max: Float) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let mut stack = [0u32; 64];
        let mut sp = 0usize;
        let mut node_idx = 0usize;

        loop {
            let node = &self.nodes[node_idx];
            if node.bbox.hit(ray, t_min, t_max) {
                if node.n_prims > 0 {
                    let start = node.offset as usize;
                    for p in &self.prims[start..start + node.n_prims as usize] {
                        if p.hit(ray, t_min, t_max).is_some() {
                            return true;
                        }
                    }
                    if sp == 0 {
                        break;
                    }
                    sp -= 1;
                    node_idx = stack[sp] as usize;
                } else {
                    stack[sp] = node.offset; // far child
                    sp += 1;
                    node_idx += 1; // near child
                }
            } else {
                if sp == 0 {
                    break;
                }
                sp -= 1;
                node_idx = stack[sp] as usize;
            }
        }
        false
    }
}

/// Recursively build the subtree for `info`, returning the index of the node
/// it created. The node is pushed first, so its first child is always at
/// `returned_index + 1`.
fn build_recursive(
    source: &[Primitive],
    info: &mut [BuildInfo],
    nodes: &mut Vec<LinearNode>,
    ordered: &mut Vec<Primitive>,
) -> usize {
    let bounds = info.iter().fold(Aabb::EMPTY, |b, p| b.union(p.bbox));

    let node_index = nodes.len();
    nodes.push(LinearNode {
        bbox: bounds,
        offset: 0,
        n_prims: 0,
        axis: 0,
    });

    let make_leaf =
        |nodes: &mut Vec<LinearNode>, ordered: &mut Vec<Primitive>, info: &[BuildInfo]| {
            let first = ordered.len() as u32;
            for p in info.iter() {
                ordered.push(source[p.prim_index]);
            }
            nodes[node_index] = LinearNode {
                bbox: bounds,
                offset: first,
                n_prims: info.len() as u32,
                axis: 0,
            };
        };

    if info.len() <= MAX_LEAF_PRIMS {
        make_leaf(nodes, ordered, info);
        return node_index;
    }

    // Partition along the axis with the widest spread of centroids.
    let cbounds = info
        .iter()
        .fold(Aabb::EMPTY, |b, p| b.union_point(p.centroid));
    let axis = cbounds.longest_axis();
    let cmin = cbounds.min.axis(axis);
    let cmax = cbounds.max.axis(axis);

    if (cmax - cmin).abs() < Float::EPSILON {
        // All centroids coincide; splitting cannot help.
        make_leaf(nodes, ordered, info);
        return node_index;
    }

    let mid = match sah_split(info, axis, cmin, cmax, bounds.surface_area()) {
        Some(mid) => mid,
        None => {
            // SAH found no beneficial split; fall back to an even median split.
            let n = info.len();
            info.select_nth_unstable_by(n / 2, |a, b| {
                a.centroid
                    .axis(axis)
                    .partial_cmp(&b.centroid.axis(axis))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            n / 2
        }
    };

    let (left, right) = info.split_at_mut(mid);
    let _first_child = build_recursive(source, left, nodes, ordered);
    let second_child = build_recursive(source, right, nodes, ordered) as u32;

    nodes[node_index] = LinearNode {
        bbox: bounds,
        offset: second_child,
        n_prims: 0,
        axis: axis as u8,
    };
    node_index
}

/// Binned SAH partition. Returns the split index (number of primitives sent to
/// the left child) after reordering `info` in place, or `None` if no split
/// beats just keeping the primitives together in a leaf.
fn sah_split(
    info: &mut [BuildInfo],
    axis: usize,
    cmin: Float,
    cmax: Float,
    parent_sa: Float,
) -> Option<usize> {
    let n = info.len();
    let inv_extent = 1.0 / (cmax - cmin);

    let bucket_of = |centroid_axis: Float| -> usize {
        let b = (N_BUCKETS as Float * (centroid_axis - cmin) * inv_extent) as usize;
        b.min(N_BUCKETS - 1)
    };

    let mut buckets = [Bucket::default(); N_BUCKETS];
    for p in info.iter() {
        let b = bucket_of(p.centroid.axis(axis));
        buckets[b].count += 1;
        buckets[b].bounds = buckets[b].bounds.union(p.bbox);
    }

    // Cost of splitting after each of the N_BUCKETS-1 boundaries.
    let mut best_cost = Float::INFINITY;
    let mut best_split = 0usize;
    for i in 0..N_BUCKETS - 1 {
        let mut left = Aabb::EMPTY;
        let mut left_count = 0u32;
        for b in &buckets[..=i] {
            left = left.union(b.bounds);
            left_count += b.count;
        }
        let mut right = Aabb::EMPTY;
        let mut right_count = 0u32;
        for b in &buckets[i + 1..] {
            right = right.union(b.bounds);
            right_count += b.count;
        }
        if left_count == 0 || right_count == 0 {
            continue;
        }
        let cost = TRAVERSAL_COST
            + (left_count as Float * left.surface_area()
                + right_count as Float * right.surface_area())
                / parent_sa;
        if cost < best_cost {
            best_cost = cost;
            best_split = i;
        }
    }

    let leaf_cost = n as Float;
    if best_cost >= leaf_cost && n <= 16 {
        // Cheaper (or comparable) to keep as a leaf, and the leaf is small.
        return None;
    }

    // Partition primitives by which side of `best_split` their bucket lands on.
    let mut i = 0;
    for j in 0..info.len() {
        if bucket_of(info[j].centroid.axis(axis)) <= best_split {
            info.swap(i, j);
            i += 1;
        }
    }

    if i == 0 || i == n {
        None
    } else {
        Some(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Sphere;

    /// Brute-force nearest hit, for cross-checking the BVH.
    fn naive_hit(prims: &[Primitive], ray: &Ray) -> Option<HitRecord> {
        let mut closest = Float::INFINITY;
        let mut best = None;
        for p in prims {
            if let Some(h) = p.hit(ray, 0.001, closest) {
                closest = h.t;
                best = Some(h);
            }
        }
        best
    }

    #[test]
    fn bvh_matches_brute_force() {
        // A grid of spheres.
        let mut prims = Vec::new();
        for x in -3..=3 {
            for y in -3..=3 {
                prims.push(Primitive::Sphere(Sphere::new(
                    Vec3::new(x as Float * 2.0, y as Float * 2.0, 0.0),
                    0.5,
                    0,
                )));
            }
        }
        let bvh = Bvh::build(prims.clone());

        let mut rng = 12345u64;
        let mut next = || {
            // xorshift for deterministic pseudo-randomness without deps
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng as Float / u64::MAX as Float) * 2.0 - 1.0
        };

        for _ in 0..2000 {
            let origin = Vec3::new(next() * 8.0, next() * 8.0, -10.0);
            let target = Vec3::new(next() * 8.0, next() * 8.0, 0.0);
            let ray = Ray::new(origin, (target - origin).normalize());

            let a = bvh.hit(&ray, 0.001, Float::INFINITY);
            let b = naive_hit(&prims, &ray);
            match (a, b) {
                (Some(ha), Some(hb)) => {
                    assert!(
                        (ha.t - hb.t).abs() < 1e-3,
                        "t mismatch: {} vs {}",
                        ha.t,
                        hb.t
                    )
                }
                (None, None) => {}
                _ => panic!("hit/miss disagreement between BVH and brute force"),
            }
        }
    }

    #[test]
    fn flatten_is_self_consistent() {
        let mut prims = vec![Primitive::Sphere(Sphere::new(Vec3::ZERO, 1.0, 0))];
        for i in 0..10 {
            let x = i as Float;
            prims.push(Primitive::Triangle(crate::geometry::Triangle::new(
                Vec3::new(x, 0.0, 0.0),
                Vec3::new(x + 1.0, 0.0, 0.0),
                Vec3::new(x, 1.0, 0.0),
                1,
            )));
        }
        let n = prims.len();
        let flat = Bvh::build(prims).flatten();

        assert_eq!(flat.prim_count, n);
        assert_eq!(flat.node_bounds.len(), flat.node_count * 6);
        assert_eq!(flat.node_meta.len(), flat.node_count * 3);
        assert_eq!(flat.prim_kind.len(), n);
        assert_eq!(flat.prim_data.len(), n * 9);
        assert_eq!(flat.prim_uv.len(), n * 6);
        assert_eq!(flat.prim_material.len(), n);
        // Every leaf's primitive range stays in bounds.
        for node in 0..flat.node_count {
            let (offset, n_prims) = (flat.node_meta[node * 3], flat.node_meta[node * 3 + 1]);
            if n_prims > 0 {
                assert!((offset + n_prims) as usize <= n, "leaf range out of bounds");
            } else {
                assert!(
                    (offset as usize) < flat.node_count,
                    "child index out of bounds"
                );
            }
        }
    }

    #[test]
    fn coplanar_tie_prefers_the_front_facing_triangle() {
        // Two identical triangles in the z = 0 plane with opposite windings,
        // as left behind by a union of two touching solids: material 0 faces
        // the ray (front), material 1 faces away (the buried interior face).
        // Whichever order they land in the BVH, the front face must win.
        let a = Vec3::new(-1.0, -1.0, 0.0);
        let b = Vec3::new(1.0, -1.0, 0.0);
        let c = Vec3::new(0.0, 1.0, 0.0);
        // (a, c, b) winds to a -z normal: towards the ray origin below.
        let front = Primitive::Triangle(crate::geometry::Triangle::new(a, c, b, 0));
        let back = Primitive::Triangle(crate::geometry::Triangle::new(a, b, c, 1));
        let ray = Ray::new(Vec3::new(0.0, 0.0, -3.0), Vec3::new(0.0, 0.0, 1.0));

        for prims in [vec![front, back], vec![back, front]] {
            let h = Bvh::build(prims)
                .hit(&ray, 0.001, Float::INFINITY)
                .expect("should hit");
            assert_eq!(h.material, 0, "the back-facing duplicate won the tie");
            assert!(h.front_face);
        }
    }

    #[test]
    fn empty_bvh_never_hits() {
        let bvh = Bvh::build(Vec::new());
        let ray = Ray::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0));
        assert!(bvh.hit(&ray, 0.001, Float::INFINITY).is_none());
    }

    #[test]
    fn occluded_respects_range_and_misses() {
        // Unit sphere at the origin; a +z ray from z=-5 hits it at t in [4, 6].
        let bvh = Bvh::build(vec![Primitive::Sphere(Sphere::new(Vec3::ZERO, 1.0, 0))]);
        let ray = Ray::new(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(bvh.occluded(&ray, 0.001, 10.0), "blocker within range");
        assert!(!bvh.occluded(&ray, 0.001, 3.0), "blocker is beyond t_max");

        let miss = Ray::new(Vec3::new(5.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(!bvh.occluded(&miss, 0.001, 100.0), "ray misses entirely");
    }
}
