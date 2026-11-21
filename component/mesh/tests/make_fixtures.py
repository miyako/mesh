"""Small original mesh fixtures: ASCII STL and PLY solids, no third-party content."""
from pathlib import Path
import struct


def triangle_stl():
    """One-facet ASCII STL: unit triangle at z=0, normal +Z."""
    normal = (0.0, 0.0, 1.0)
    vertices = ((0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0))
    lines = ["solid triangle"]
    lines.append("  facet normal %g %g %g" % normal)
    lines.append("    outer loop")
    for point in vertices:
        lines.append("      vertex %g %g %g" % point)
    lines.append("    endloop")
    lines.append("  endfacet")
    lines.append("endsolid triangle")
    return ("\n".join(lines) + "\n").encode("ascii")


def cube_ply():
    """ASCII PLY unit cube: 8 vertices, 12 triangles."""
    vertices = [(x, y, z) for x in (0.0, 1.0) for y in (0.0, 1.0) for z in (0.0, 1.0)]
    quads = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1),
             (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]
    triangles = []
    for a, b, c, d in quads:
        triangles.append((a, b, c))
        triangles.append((a, c, d))
    lines = [
        "ply",
        "format ascii 1.0",
        "element vertex 8",
        "property float x",
        "property float y",
        "property float z",
        "element face 12",
        "property list uchar int vertex_indices",
        "end_header",
    ]
    for point in vertices:
        lines.append("%g %g %g" % point)
    for triangle in triangles:
        lines.append("3 %d %d %d" % triangle)
    return ("\n".join(lines) + "\n").encode("ascii")


def malformed_stl():
    """Not an STL at all: render must fail with a real error envelope."""
    return b"this is not a mesh file\n"


def write_fixtures(destination):
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    fixtures = {
        "triangle.stl": triangle_stl(),
        "cube.ply": cube_ply(),
        "malformed.stl": malformed_stl(),
    }
    for name, content in fixtures.items():
        (destination / name).write_bytes(content)


if __name__ == "__main__":
    write_fixtures(Path(__file__).parent / "host/Resources")