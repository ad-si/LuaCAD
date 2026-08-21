// The six chess pieces, from quaternionmedia/scad-chess (CC-BY-4.0).
//
// Each piece is a lathe: a profile drawn in Inkscape, imported from SVG and
// spun around the Z axis. The rook loses turret crenellations to a boolean
// and the bishop its mitre slot; the knight and queen carry an imported STL
// on top of the lathed body.
//
// Every piece stands on z = 0 and is 20 mm across at the base, so board.scad's
// 32 mm square leaves 6 mm of margin all round.
//
// The profile paths are written as chess.scad sees them: LuaCAD resolves an
// import() against the directory of the file it was asked to render, not the
// file the call sits in. Render chess.scad, not this.

module pawn(scale = 1, segments = 64) {
  scale(scale)
    rotate_extrude(convexity = 10, $fn = segments)
      import(file = "profiles/pawn_profile.svg");
}

// The turret is cut by `cutouts` wedges spun around the top of the barrel.
module rook(scale = 1, segments = 64, cutouts = 6) {
  turret_height = 10;

  scale(scale) difference() {
    rotate_extrude(convexity = 10, $fn = segments)
      import(file = "profiles/rook_profile.svg");

    translate([0, 0, 33])
      for (i = [0 : cutouts - 1])
        rotate([0, 0, 360 / cutouts * i])
          linear_extrude(height = turret_height)
            polygon(points = [[0, 0], [11, 2], [11, -2]]);
  }
}

// The mitre's slot is a thin slab pushed through the head at 60 degrees.
module bishop(scale = 1, segments = 64) {
  scale(scale) rotate([0, 0, -90]) difference() {
    rotate_extrude(convexity = 10, $fn = segments)
      import(file = "profiles/bishop_profile.svg");

    translate([9, 0, 34])
      rotate([0, 60, 0])
        cube([2, 20, 20], center = true);
  }
}

// `dfix` widens the head to compensate at low segment counts, where the
// lathed collar it sits in comes out narrower than the profile.
module knight(scale = 1, segments = 64, dfix = 1) {
  scale(scale) rotate([0, 0, -90]) union() {
    rotate_extrude(convexity = 10, $fn = segments)
      import(file = "profiles/knight_profile.svg");

    translate([0, 0, 8])
      scale([0.47 * dfix, 0.47 * dfix, 0.47])
        translate([-2.6, -4, 0])
          import(file = "profiles/horse3.stl");
  }
}

// `rfix` turns the crown so its points miss the body's facets.
module queen(scale = 1, segments = 64, rfix = 15) {
  scale(scale) union() {
    rotate_extrude(convexity = 10, $fn = segments)
      import(file = "profiles/queen_profile.svg");

    translate([0, 0, 30.5])
      rotate([0, 0, rfix])
        scale(0.9)
          import(file = "profiles/queen_crown2.stl");
  }
}

// The cross is a flat extrusion standing in the XZ plane, not a lathe.
module king(scale = 1, segments = 64) {
  scale(scale) union() {
    rotate_extrude(convexity = 10, $fn = segments)
      import(file = "profiles/king_profile_body.svg");

    rotate([90, 0, 0])
      translate([0, 0, -1.5])
        linear_extrude(height = 3)
          import(file = "profiles/king_profile_cross.svg");
  }
}
