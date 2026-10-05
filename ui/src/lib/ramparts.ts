// Decodes memory part numbers into plain facts. Only fields whose meaning
// is documented by the maker are decoded; anything else is left out.

type Facts = [string, string][];

// G.Skill: F4-3600C16D-16GVKC  /  per-module SPD: F4-3600C16-8GVK
//   F4 = DDR4 (F5 = DDR5), 3600 = MT/s, C16 = CAS latency 16,
//   D/Q = dual/quad kit, 16G = capacity (kit total, or per module on the
//   module's own SPD), then the series code (V = Ripjaws V, TZ = Trident Z...).
const GSKILL = /^F([345])-(\d{4})C(\d{2})([DQ]?)-(\d+)G([A-Z]+)$/;
const GSKILL_SERIES: [string, string][] = [
  ["TZR", "Trident Z RGB"],
  ["TZN", "Trident Z Neo"],
  ["TZ", "Trident Z"],
  ["V", "Ripjaws V"],
];

export function decodeRamPart(part: string): Facts | null {
  const g = part.trim().toUpperCase().match(GSKILL);
  if (g) {
    const [, gen, speed, cl, kit, size, series] = g;
    const facts: Facts = [
      ["Generation", `DDR${gen}`],
      ["Rated speed", `${speed} MT/s`],
      ["CAS latency", `CL${Number(cl)}`],
      [kit ? "Kit size" : "Module size", `${size} GB`],
    ];
    const name = GSKILL_SERIES.find(([code]) => series.startsWith(code))?.[1];
    if (name) facts.push(["Series", `G.Skill ${name}`]);
    return facts;
  }
  return null;
}
