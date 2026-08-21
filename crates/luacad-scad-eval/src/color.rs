//! `color()` value parsing: named CSS/OpenSCAD colors, `#rgb[a]`/`#rrggbb[aa]`
//! hex, and `[r,g,b(,a)]` vectors (components 0..1). Returns linear-ish RGBA in
//! 0..1 (OpenSCAD passes the components straight through; we don't gamma-correct).

use crate::value::Value;

/// Parse a `color()` argument into RGBA (0..1). `alpha` (the optional second
/// positional / `alpha=`) overrides any alpha carried by `c`. Returns `None` for
/// an unrecognized value so the caller can warn and fall back to the default.
pub fn parse_color(c: &Value, alpha: Option<f64>) -> Option<[f32; 4]> {
    let mut rgba = match c {
        Value::Str(s) => parse_color_str(s)?,
        Value::Vector(els) => {
            let comp = |i: usize| els.get(i).and_then(Value::as_number);
            let r = comp(0)?;
            let g = comp(1)?;
            let b = comp(2)?;
            let a = comp(3).unwrap_or(1.0);
            [r as f32, g as f32, b as f32, a as f32]
        }
        _ => return None,
    };
    if let Some(a) = alpha {
        rgba[3] = a as f32;
    }
    for x in &mut rgba {
        *x = x.clamp(0.0, 1.0);
    }
    Some(rgba)
}

fn parse_color_str(s: &str) -> Option<[f32; 4]> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    // Named colors are case-insensitive.
    let lower = s.to_ascii_lowercase();
    let [r, g, b] = named(&lower)?;
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
}

fn parse_hex(hex: &str) -> Option<[f32; 4]> {
    let bytes = hex.as_bytes();
    let nib = |c: u8| (c as char).to_digit(16);
    match bytes.len() {
        // #rgb / #rgba — each nibble doubled.
        3 | 4 => {
            let mut out = [1.0f32; 4];
            for (i, &c) in bytes.iter().enumerate() {
                let n = nib(c)?;
                out[i] = (n * 16 + n) as f32 / 255.0;
            }
            Some(out)
        }
        // #rrggbb / #rrggbbaa
        6 | 8 => {
            let mut out = [1.0f32; 4];
            for i in 0..(bytes.len() / 2) {
                let hi = nib(bytes[i * 2])?;
                let lo = nib(bytes[i * 2 + 1])?;
                out[i] = (hi * 16 + lo) as f32 / 255.0;
            }
            Some(out)
        }
        _ => None,
    }
}

/// The CSS3 / SVG named colors (the set OpenSCAD's `color("name")` accepts),
/// lowercased. Returns 8-bit RGB.
fn named(name: &str) -> Option<[u8; 3]> {
    let rgb = match name {
        "aliceblue" => [240, 248, 255],
        "antiquewhite" => [250, 235, 215],
        "aqua" => [0, 255, 255],
        "aquamarine" => [127, 255, 212],
        "azure" => [240, 255, 255],
        "beige" => [245, 245, 220],
        "bisque" => [255, 228, 196],
        "black" => [0, 0, 0],
        "blanchedalmond" => [255, 235, 205],
        "blue" => [0, 0, 255],
        "blueviolet" => [138, 43, 226],
        "brown" => [165, 42, 42],
        "burlywood" => [222, 184, 135],
        "cadetblue" => [95, 158, 160],
        "chartreuse" => [127, 255, 0],
        "chocolate" => [210, 105, 30],
        "coral" => [255, 127, 80],
        "cornflowerblue" => [100, 149, 237],
        "cornsilk" => [255, 248, 220],
        "crimson" => [220, 20, 60],
        "cyan" => [0, 255, 255],
        "darkblue" => [0, 0, 139],
        "darkcyan" => [0, 139, 139],
        "darkgoldenrod" => [184, 134, 11],
        "darkgray" | "darkgrey" => [169, 169, 169],
        "darkgreen" => [0, 100, 0],
        "darkkhaki" => [189, 183, 107],
        "darkmagenta" => [139, 0, 139],
        "darkolivegreen" => [85, 107, 47],
        "darkorange" => [255, 140, 0],
        "darkorchid" => [153, 50, 204],
        "darkred" => [139, 0, 0],
        "darksalmon" => [233, 150, 122],
        "darkseagreen" => [143, 188, 143],
        "darkslateblue" => [72, 61, 139],
        "darkslategray" | "darkslategrey" => [47, 79, 79],
        "darkturquoise" => [0, 206, 209],
        "darkviolet" => [148, 0, 211],
        "deeppink" => [255, 20, 147],
        "deepskyblue" => [0, 191, 255],
        "dimgray" | "dimgrey" => [105, 105, 105],
        "dodgerblue" => [30, 144, 255],
        "firebrick" => [178, 34, 34],
        "floralwhite" => [255, 250, 240],
        "forestgreen" => [34, 139, 34],
        "fuchsia" => [255, 0, 255],
        "gainsboro" => [220, 220, 220],
        "ghostwhite" => [248, 248, 255],
        "gold" => [255, 215, 0],
        "goldenrod" => [218, 165, 32],
        "gray" | "grey" => [128, 128, 128],
        "green" => [0, 128, 0],
        "greenyellow" => [173, 255, 47],
        "honeydew" => [240, 255, 240],
        "hotpink" => [255, 105, 180],
        "indianred" => [205, 92, 92],
        "indigo" => [75, 0, 130],
        "ivory" => [255, 255, 240],
        "khaki" => [240, 230, 140],
        "lavender" => [230, 230, 250],
        "lavenderblush" => [255, 240, 245],
        "lawngreen" => [124, 252, 0],
        "lemonchiffon" => [255, 250, 205],
        "lightblue" => [173, 216, 230],
        "lightcoral" => [240, 128, 128],
        "lightcyan" => [224, 255, 255],
        "lightgoldenrodyellow" => [250, 250, 210],
        "lightgray" | "lightgrey" => [211, 211, 211],
        "lightgreen" => [144, 238, 144],
        "lightpink" => [255, 182, 193],
        "lightsalmon" => [255, 160, 122],
        "lightseagreen" => [32, 178, 170],
        "lightskyblue" => [135, 206, 250],
        "lightslategray" | "lightslategrey" => [119, 136, 153],
        "lightsteelblue" => [176, 196, 222],
        "lightyellow" => [255, 255, 224],
        "lime" => [0, 255, 0],
        "limegreen" => [50, 205, 50],
        "linen" => [250, 240, 230],
        "magenta" => [255, 0, 255],
        "maroon" => [128, 0, 0],
        "mediumaquamarine" => [102, 205, 170],
        "mediumblue" => [0, 0, 205],
        "mediumorchid" => [186, 85, 211],
        "mediumpurple" => [147, 112, 219],
        "mediumseagreen" => [60, 179, 113],
        "mediumslateblue" => [123, 104, 238],
        "mediumspringgreen" => [0, 250, 154],
        "mediumturquoise" => [72, 209, 204],
        "mediumvioletred" => [199, 21, 133],
        "midnightblue" => [25, 25, 112],
        "mintcream" => [245, 255, 250],
        "mistyrose" => [255, 228, 225],
        "moccasin" => [255, 228, 181],
        "navajowhite" => [255, 222, 173],
        "navy" => [0, 0, 128],
        "oldlace" => [253, 245, 230],
        "olive" => [128, 128, 0],
        "olivedrab" => [107, 142, 35],
        "orange" => [255, 165, 0],
        "orangered" => [255, 69, 0],
        "orchid" => [218, 112, 214],
        "palegoldenrod" => [238, 232, 170],
        "palegreen" => [152, 251, 152],
        "paleturquoise" => [175, 238, 238],
        "palevioletred" => [219, 112, 147],
        "papayawhip" => [255, 239, 213],
        "peachpuff" => [255, 218, 185],
        "peru" => [205, 133, 63],
        "pink" => [255, 192, 203],
        "plum" => [221, 160, 221],
        "powderblue" => [176, 224, 230],
        "purple" => [128, 0, 128],
        "rebeccapurple" => [102, 51, 153],
        "red" => [255, 0, 0],
        "rosybrown" => [188, 143, 143],
        "royalblue" => [65, 105, 225],
        "saddlebrown" => [139, 69, 19],
        "salmon" => [250, 128, 114],
        "sandybrown" => [244, 164, 96],
        "seagreen" => [46, 139, 87],
        "seashell" => [255, 245, 238],
        "sienna" => [160, 82, 45],
        "silver" => [192, 192, 192],
        "skyblue" => [135, 206, 235],
        "slateblue" => [106, 90, 205],
        "slategray" | "slategrey" => [112, 128, 144],
        "snow" => [255, 250, 250],
        "springgreen" => [0, 255, 127],
        "steelblue" => [70, 130, 180],
        "tan" => [210, 180, 140],
        "teal" => [0, 128, 128],
        "thistle" => [216, 191, 216],
        "tomato" => [255, 99, 71],
        "turquoise" => [64, 224, 208],
        "violet" => [238, 130, 238],
        "wheat" => [245, 222, 179],
        "white" => [255, 255, 255],
        "whitesmoke" => [245, 245, 245],
        "yellow" => [255, 255, 0],
        "yellowgreen" => [154, 205, 50],
        _ => return None,
    };
    Some(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::vector;

    fn v(xs: &[f64]) -> Value {
        vector(xs.iter().map(|n| Value::Number(*n)).collect())
    }

    #[test]
    fn named_hex_vector_alpha() {
        assert_eq!(
            parse_color(&Value::Str("red".into()), None),
            Some([1.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(
            parse_color(&Value::Str("RED".into()), None),
            Some([1.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(
            parse_color(&Value::Str("#00ff00".into()), None),
            Some([0.0, 1.0, 0.0, 1.0])
        );
        // short hex with alpha
        assert_eq!(
            parse_color(&Value::Str("#0f08".into()), None),
            Some([0.0, 1.0, 0.0, 0.53333336])
        );
        // vector rgb + explicit alpha override
        assert_eq!(
            parse_color(&v(&[0.0, 0.0, 1.0]), Some(0.5)),
            Some([0.0, 0.0, 1.0, 0.5])
        );
        // vector rgba, alpha from the 4th component
        assert_eq!(
            parse_color(&v(&[0.0, 0.0, 1.0, 0.25]), None),
            Some([0.0, 0.0, 1.0, 0.25])
        );
        // unknown → None
        assert_eq!(parse_color(&Value::Str("notacolor".into()), None), None);
        assert_eq!(parse_color(&Value::Number(5.0), None), None);
    }
}
