## 2025-09-07 - Avoid std::fmt::Write in hot loop SVG path formatting
**Learning:** Using `write!(d, "M{} {}", x, y)` or similar formatting macros in tight path serialization loops triggers macro string parsing and formatting overhead. Custom direct ASCII formatting or `itoa` formatting reduces SVG path emission overhead by ~30%.
**Action:** Replace `write!` macro calls in SVG string path generation loops with direct byte/ASCII conversions or `itoa` formatting.
