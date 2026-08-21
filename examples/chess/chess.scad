// A chess game in progress, set up from a FEN string.
//
// The position is the Immortal Game — Anderssen vs Kieseritzky, London 1851 —
// after Black's 22...Nxf6. White has given up a bishop, both rooks and the
// queen, and mates next move with 23.Be7#. Black's queen sits on a1 and his
// bishop on g1, on the squares the rooks came from.
//
//   luacad render --raytrace chess.scad chess_raytraced.png
//   luacad render            chess.scad chess.png
//   luacad convert           chess.scad chess.3mf
//
// Drop any other FEN in POSITION below to set up a different game. Only the
// piece-placement field is read; the side to move and the castling, en
// passant and clock fields are ignored, so either form works.

use <parts/pieces.scad>
include <parts/board.scad>

POSITION = "r1bk3r/p2p1pNp/n2B1n2/1p1NP2P/6P1/3P4/P1P1K3/q5b1 w - - 0 23";

SEGMENTS = 64;           // facets around each lathed piece
CUTOUTS = 6;             // crenellations on a rook

WHITE = "#f2e8d5";       // bone
BLACK = "#2b2724";       // ebony


// ---------------------------------------------------------------------------
// Reading FEN
//
// The placement field lists ranks 8 down to 1, separated by "/". A letter is
// a piece — uppercase White, lowercase Black — and a digit is that many empty
// squares. Everything after the first space is the game state, not the board.
// ---------------------------------------------------------------------------

function is_digit(c) = len(search(c, "12345678")) > 0;
function digit(c) = search(c, "0123456789")[0];
function is_white(c) = len(search(c, "PNBRQK")) > 0;

// Fold the two cases together: index 6-11 wraps back onto 0-5.
function kind(c) =
  ["p", "n", "b", "r", "q", "k"][search(c, "pnbrqkPNBRQK")[0] % 6];

// Walk the placement field one character at a time, emitting
// [file, rank, letter] for each piece. `f` counts files a-h left to right,
// `r` counts ranks down from 7 (rank 8) as each "/" is crossed.
function placement(fen, i = 0, f = 0, r = 7) =
    i >= len(fen) || fen[i] == " " ? []
  : fen[i] == "/"                  ? placement(fen, i + 1, 0, r - 1)
  : is_digit(fen[i])               ? placement(fen, i + 1, f + digit(fen[i]), r)
  : concat([[f, r, fen[i]]], placement(fen, i + 1, f + 1, r));

// Algebraic name of a square — echoed on load, so the parse can be read back
// against the diagram in README.md.
function square_name(f, r) = str("abcdefgh"[f], r + 1);

for (p = placement(POSITION))
  echo(str(p[2], square_name(p[0], p[1])));


// ---------------------------------------------------------------------------
// Building the position
// ---------------------------------------------------------------------------

module piece(letter) {
  k = kind(letter);

  if (k == "p") pawn(1, SEGMENTS);
  else if (k == "r") rook(1, SEGMENTS, CUTOUTS);
  else if (k == "n") knight(1, SEGMENTS);
  else if (k == "b") bishop(1, SEGMENTS);
  else if (k == "q") queen(1, SEGMENTS);
  else if (k == "k") king(1, SEGMENTS);
}

module position(fen) {
  for (p = placement(fen)) {
    f = p[0];
    r = p[1];
    letter = p[2];
    white = is_white(letter);

    color(white ? WHITE : BLACK)
      translate([square_x(f), square_y(r), 0])
        // Black's men turn to face down the board, so the two knights and
        // the two bishops look at each other rather than the same way.
        rotate([0, 0, white ? 0 : 180])
          piece(letter);
  }
}

board();
position(POSITION);
