// The board: 64 tiles inlaid in a frame, playing surface flush with z = 0.
//
// a1 is at file 0, rank 0, and comes out dark — as it must, since every
// board is laid out "white on the right".

SQUARE = 32;             // side of one square, matching the 20 mm pieces
BOARD = 8 * SQUARE;      // 256 mm of playing surface
TILE = 4;                // depth of the inlaid tiles
SLAB = 10;               // total thickness under the playing surface
RIM = 20;                // width of the frame around the tiles
RIM_RISE = 2;            // how far the frame stands proud of the tiles

LIGHT = "#e3cfa4";       // maple
DARK = "#6d4a33";        // walnut
FRAME = "#432b1d";       // darker walnut

// Centre of the square at file f, rank r (both 0-7).
function square_x(f) = (f - 3.5) * SQUARE;
function square_y(r) = (r - 3.5) * SQUARE;

module tiles() {
  for (f = [0 : 7])
    for (r = [0 : 7])
      color((f + r) % 2 == 0 ? DARK : LIGHT)
        translate([square_x(f) - SQUARE / 2, square_y(r) - SQUARE / 2, -TILE])
          cube([SQUARE, SQUARE, TILE]);
}

module frame() {
  color(FRAME) difference() {
    // Outer block: the base slab plus the raised rim in one piece.
    translate([-BOARD / 2 - RIM, -BOARD / 2 - RIM, -SLAB])
      cube([BOARD + 2 * RIM, BOARD + 2 * RIM, SLAB + RIM_RISE]);

    // Recess the tiles sit in. Cut from the top down to -TILE only, so the
    // slab below stays solid and carries them.
    translate([-BOARD / 2, -BOARD / 2, -TILE])
      cube([BOARD, BOARD, TILE + RIM_RISE + 1]);
  }
}

module board() {
  tiles();
  frame();
}
