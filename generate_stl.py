#!/usr/bin/env python3
"""Generate ASCII STL files for the weather station enclosure.

Output:
  front_panel.stl  — 前面板（屏幕窗口 + 触摸区薄壁）
  bottom_shell.stl — 底壳（USB 口 + 透气孔 + 蜂鸣器孔）

Units: millimeters
"""

# === Dimensions ===
OUTER_X, OUTER_Y, OUTER_Z = 100.0, 80.0, 40.0  # mm
WALL = 2.0
INNER_X = OUTER_X - 2 * WALL  # 96
INNER_Y = OUTER_Y - 2 * WALL  # 76
INNER_Z = OUTER_Z - WALL       # 38 (open top, 2mm floor)

SCREEN_W, SCREEN_H = 75.0, 51.0   # screen window
TOUCH_D = 20.0                      # touch circle diameter
TOUCH_THICK = 0.8                   # thin wall for touch area

USB_W, USB_H = 9.0, 4.0            # USB-C opening
VENT_D = 6.0                        # vent hole diameter
BUZZER_D = 4.0                      # buzzer hole diameter
N_HOLES = 3                         # number of buzzer holes

# Center positions (relative to outer box origin at corner)
SCREEN_CX = OUTER_X / 2
SCREEN_CY = OUTER_Y / 2 + 5        # slightly above center
TOUCH_CX = OUTER_X / 2
TOUCH_CY = OUTER_Y - 18            # near bottom of front face

USB_CX = OUTER_X / 2
USB_CY = WALL + 1.5                 # on back face, near bottom
VENT_CX = 25.0
VENT_CY = OUTER_Y / 2              # bottom face, left side
BUZZER_START_X = OUTER_X - 30.0
BUZZER_CY = OUTER_Y / 2            # right side of bottom face


class AsciiStl:
    def __init__(self, name):
        self.name = name
        self.triangles = []

    def add_tri(self, v1, v2, v3, nx=0, ny=0, nz=0):
        self.triangles.append((v1, v2, v3, (nx, ny, nz)))

    def add_box_faces(self, x0, y0, z0, x1, y1, z1):
        """Add 12 triangles (6 faces) for a box from (x0,y0,z0) to (x1,y1,z1)."""
        # -Z face
        self.add_tri((x0,y0,z0), (x1,y0,z0), (x1,y1,z0), 0,0,-1)
        self.add_tri((x0,y0,z0), (x1,y1,z0), (x0,y1,z0), 0,0,-1)
        # +Z face
        self.add_tri((x0,y0,z1), (x1,y1,z1), (x1,y0,z1), 0,0,1)
        self.add_tri((x0,y0,z1), (x0,y1,z1), (x1,y1,z1), 0,0,1)
        # -Y face
        self.add_tri((x0,y0,z0), (x1,y0,z1), (x1,y0,z0), 0,-1,0)
        self.add_tri((x0,y0,z0), (x0,y0,z1), (x1,y0,z1), 0,-1,0)
        # +Y face
        self.add_tri((x0,y1,z0), (x1,y1,z0), (x1,y1,z1), 0,1,0)
        self.add_tri((x0,y1,z0), (x1,y1,z1), (x0,y1,z1), 0,1,0)
        # -X face
        self.add_tri((x0,y0,z0), (x0,y1,z0), (x0,y1,z1), -1,0,0)
        self.add_tri((x0,y0,z0), (x0,y1,z1), (x0,y0,z1), -1,0,0)
        # +X face
        self.add_tri((x1,y0,z0), (x1,y0,z1), (x1,y1,z1), 1,0,0)
        self.add_tri((x1,y0,z0), (x1,y1,z1), (x1,y1,z0), 1,0,0)

    def write(self, filename):
        with open(filename, 'w') as f:
            f.write(f"solid {self.name}\n")
            for v1, v2, v3, n in self.triangles:
                f.write(f"  facet normal {n[0]} {n[1]} {n[2]}\n")
                f.write(f"    outer loop\n")
                f.write(f"      vertex {v1[0]} {v1[1]} {v1[2]}\n")
                f.write(f"      vertex {v2[0]} {v2[1]} {v2[2]}\n")
                f.write(f"      vertex {v3[0]} {v3[1]} {v3[2]}\n")
                f.write(f"    endloop\n")
                f.write(f"  endfacet\n")
            f.write(f"endsolid {self.name}\n")
        print(f"  → {filename} ({len(self.triangles)} triangles)")


def make_box_with_rect_hole(stl, x0, y0, z0, x1, y1, z1, hx0, hy0, hx1, hy1, face='z'):
    """Box with a rectangular hole on the specified face.
    face='z' means the hole is on the z=z1 face (top of box)."""
    if face == 'z':
        # Bottom face (-Z): solid
        stl.add_tri((x0,y0,z0), (x1,y0,z0), (x1,y1,z0), 0,0,-1)
        stl.add_tri((x0,y0,z0), (x1,y1,z0), (x0,y1,z0), 0,0,-1)
        # Top face (+Z): ring with rectangular hole
        add_rect_ring(stl, x0,y0,z1, x1,y1,z1, hx0,hy0,hx1,hy1, 0,0,1)
        # Side faces (4 faces, full height)
        # -Y
        stl.add_tri((x0,y0,z0), (x1,y0,z1), (x1,y0,z0), 0,-1,0)
        stl.add_tri((x0,y0,z0), (x0,y0,z1), (x1,y0,z1), 0,-1,0)
        # +Y
        stl.add_tri((x0,y1,z0), (x1,y1,z0), (x1,y1,z1), 0,1,0)
        stl.add_tri((x0,y1,z0), (x1,y1,z1), (x0,y1,z1), 0,1,0)
        # -X
        stl.add_tri((x0,y0,z0), (x0,y1,z0), (x0,y1,z1), -1,0,0)
        stl.add_tri((x0,y0,z0), (x0,y1,z1), (x0,y0,z1), -1,0,0)
        # +X
        stl.add_tri((x1,y0,z0), (x1,y0,z1), (x1,y1,z1), 1,0,0)
        stl.add_tri((x1,y0,z0), (x1,y1,z1), (x1,y1,z0), 1,0,0)
        # Inner wall faces of the hole (4 strips)
        # -Y inner
        stl.add_tri((hx0,hy0,z0), (hx1,hy0,z1), (hx1,hy0,z0), 0,1,0)
        stl.add_tri((hx0,hy0,z0), (hx0,hy0,z1), (hx1,hy0,z1), 0,1,0)
        # +Y inner
        stl.add_tri((hx0,hy1,z0), (hx1,hy1,z0), (hx1,hy1,z1), 0,-1,0)
        stl.add_tri((hx0,hy1,z0), (hx1,hy1,z1), (hx0,hy1,z1), 0,-1,0)
        # -X inner
        stl.add_tri((hx0,hy0,z0), (hx0,hy1,z0), (hx0,hy1,z1), 1,0,0)
        stl.add_tri((hx0,hy0,z0), (hx0,hy1,z1), (hx0,hy0,z1), 1,0,0)
        # +X inner
        stl.add_tri((hx1,hy0,z0), (hx1,hy0,z1), (hx1,hy1,z1), -1,0,0)
        stl.add_tri((hx1,hy0,z0), (hx1,hy1,z1), (hx1,hy1,z0), -1,0,0)
    elif face == 'x':
        # +X face: ring with rectangular hole
        add_rect_ring_x(stl, x1,y0,z0, x1,y1,z1, hx0,hy0,hx1,hy1, 1,0,0)
        # Other 5 faces: solid
        stl.add_box_faces(x0,y0,z0, x1-0.001,y1,z1)
        # Inner wall of the hole
        stl.add_tri((x1,hy0,hx0), (x1,hy1,hx0), (x1,hy1,hx1), -1,0,0)
        stl.add_tri((x1,hy0,hx0), (x1,hy1,hx1), (x1,hy0,hx1), -1,0,0)


def add_rect_ring(stl, x0,y0,z, x1,y1,z2, hx0,hy0,hx1,hy1, nx,ny,nz):
    """Add 4 triangles forming a rectangular ring on z=z face."""
    # Triangle 1: (x0,y0) - (hx0,y0) - (hx0,hy0)
    stl.add_tri((x0,y0,z), (hx0,y0,z), (hx0,hy0,z), nx,ny,nz)
    # Triangle 2: (x0,y0) - (hx0,hy0) - (x0,hy0)
    stl.add_tri((x0,y0,z), (hx0,hy0,z), (x0,hy0,z), nx,ny,nz)
    # Triangle 3: (hx1,y0) - (x1,y0) - (x1,hy0)
    stl.add_tri((hx1,y0,z), (x1,y0,z), (x1,hy0,z), nx,ny,nz)
    # Triangle 4: (hx1,y0) - (x1,hy0) - (hx1,hy0)
    stl.add_tri((hx1,y0,z), (x1,hy0,z), (hx1,hy0,z), nx,ny,nz)
    # Triangle 5: (x0,hy1) - (hx0,hy1) - (hx0,y1)
    stl.add_tri((x0,hy1,z), (hx0,hy1,z), (hx0,y1,z), nx,ny,nz)
    # Triangle 6: (x0,hy1) - (hx0,y1) - (x0,y1)
    stl.add_tri((x0,hy1,z), (hx0,y1,z), (x0,y1,z), nx,ny,nz)
    # Triangle 7: (hx1,hy1) - (x1,hy1) - (x1,y1)
    stl.add_tri((hx1,hy1,z), (x1,hy1,z), (x1,y1,z), nx,ny,nz)
    # Triangle 8: (hx1,hy1) - (x1,y1) - (hx1,y1)
    stl.add_tri((hx1,hy1,z), (x1,y1,z), (hx1,y1,z), nx,ny,nz)


def add_rect_ring_y(stl, x0,y,x0z, x1,y1z, hx0,hz0,hx1,hz1, nx,ny,nz):
    """Add 4 triangles for a rectangular ring on y=y face (XZ plane)."""
    # Bottom strip: x0-hx0
    stl.add_tri((x0,y,x0z), (hx0,y,x0z), (hx0,y,hz0), nx,ny,nz)
    stl.add_tri((x0,y,x0z), (hx0,y,hz0), (x0,y,hz0), nx,ny,nz)
    # Bottom strip: hx1-x1
    stl.add_tri((hx1,y,x0z), (x1,y,x0z), (x1,y,hz0), nx,ny,nz)
    stl.add_tri((hx1,y,x0z), (x1,y,hz0), (hx1,y,hz0), nx,ny,nz)
    # Top strip: x0-hx0
    stl.add_tri((x0,y,hz1), (hx0,y,hz1), (hx0,y,y1z), nx,ny,nz)
    stl.add_tri((x0,y,hz1), (hx0,y,y1z), (x0,y,y1z), nx,ny,nz)
    # Top strip: hx1-x1
    stl.add_tri((hx1,y,hz1), (x1,y,hz1), (x1,y,y1z), nx,ny,nz)
    stl.add_tri((hx1,y,hz1), (x1,y,y1z), (hx1,y,y1z), nx,ny,nz)


def generate_front_panel():
    """Front panel: outer box minus inner cavity, with screen window on front face."""
    stl = AsciiStl("front_panel")

    # Front face (y=OUTER_Y, Z-up): screen window
    sx0 = SCREEN_CX - SCREEN_W / 2
    sy0 = SCREEN_CY - SCREEN_H / 2
    sx1 = SCREEN_CX + SCREEN_W / 2
    sy1 = SCREEN_CY + SCREEN_H / 2

    # Front face (+Y): outer ring with screen hole
    add_rect_ring_y(stl, 0, OUTER_Y, 0, OUTER_X, OUTER_Z,
                     sx0, sy0, sx1, sy1, 0, 1, 0)

    # Back face (-Y): solid
    stl.add_tri((0,0,0), (OUTER_X,0,OUTER_Z), (OUTER_X,0,0), 0,-1,0)
    stl.add_tri((0,0,0), (0,0,OUTER_Z), (OUTER_X,0,OUTER_Z), 0,-1,0)

    # Top face (+Z): solid
    stl.add_tri((0,0,OUTER_Z), (OUTER_X,0,OUTER_Z), (OUTER_X,OUTER_Y,OUTER_Z), 0,0,1)
    stl.add_tri((0,0,OUTER_Z), (OUTER_X,OUTER_Y,OUTER_Z), (0,OUTER_Y,OUTER_Z), 0,0,1)

    # Bottom face (-Z): solid
    stl.add_tri((0,0,0), (OUTER_X,OUTER_Y,0), (OUTER_X,0,0), 0,0,-1)
    stl.add_tri((0,0,0), (0,OUTER_Y,0), (OUTER_X,OUTER_Y,0), 0,0,-1)

    # Left face (-X): solid
    stl.add_tri((0,0,0), (0,OUTER_Y,0), (0,OUTER_Y,OUTER_Z), -1,0,0)
    stl.add_tri((0,0,0), (0,OUTER_Y,OUTER_Z), (0,0,OUTER_Z), -1,0,0)

    # Right face (+X): solid
    stl.add_tri((OUTER_X,0,0), (OUTER_X,0,OUTER_Z), (OUTER_X,OUTER_Y,OUTER_Z), 1,0,0)
    stl.add_tri((OUTER_X,0,0), (OUTER_X,OUTER_Y,OUTER_Z), (OUTER_X,OUTER_Y,0), 1,0,0)

    # Inner wall of screen window (4 strips)
    depth = WALL
    # -Y inner (front of window)
    stl.add_tri((sx0,sy0,0), (sx1,sy0,depth), (sx1,sy0,0), 0,1,0)
    stl.add_tri((sx0,sy0,0), (sx0,sy0,depth), (sx1,sy0,depth), 0,1,0)
    # +Y inner (back of window)
    stl.add_tri((sx0,sy1,0), (sx1,sy1,0), (sx1,sy1,depth), 0,-1,0)
    stl.add_tri((sx0,sy1,0), (sx1,sy1,depth), (sx0,sy1,depth), 0,-1,0)
    # -X inner
    stl.add_tri((sx0,sy0,0), (sx0,sy1,0), (sx0,sy1,depth), 1,0,0)
    stl.add_tri((sx0,sy0,0), (sx0,sy1,depth), (sx0,sy0,depth), 1,0,0)
    # +X inner
    stl.add_tri((sx1,sy0,0), (sx1,sy0,depth), (sx1,sy1,depth), -1,0,0)
    stl.add_tri((sx1,sy0,0), (sx1,sy1,depth), (sx1,sy1,0), -1,0,0)

    stl.write("front_panel.stl")


def generate_bottom_shell():
    """Bottom shell: hollow box (open top) with USB, vent, and buzzer holes on bottom face."""
    stl = AsciiStl("bottom_shell")

    ox, oy, oz = OUTER_X, OUTER_Y, OUTER_Z
    ix, iy, iz = INNER_X, INNER_Y, INNER_Z
    w = WALL

    # --- Outer faces (5 faces, top open) ---
    # Bottom (-Z)
    stl.add_tri((0,0,0), (ox,oy,0), (ox,0,0), 0,0,-1)
    stl.add_tri((0,0,0), (0,oy,0), (ox,oy,0), 0,0,-1)
    # Front (+Y)
    stl.add_tri((0,oy,0), (ox,oy,0), (ox,oy,oz), 0,1,0)
    stl.add_tri((0,oy,0), (ox,oy,oz), (0,oy,oz), 0,1,0)
    # Back (-Y)
    stl.add_tri((0,0,0), (0,0,oz), (ox,0,oz), 0,-1,0)
    stl.add_tri((0,0,0), (ox,0,oz), (ox,0,0), 0,-1,0)
    # Left (-X)
    stl.add_tri((0,0,0), (0,oy,0), (0,oy,oz), -1,0,0)
    stl.add_tri((0,0,0), (0,oy,oz), (0,0,oz), -1,0,0)
    # Right (+X)
    stl.add_tri((ox,0,0), (ox,0,oz), (ox,oy,oz), 1,0,0)
    stl.add_tri((ox,0,0), (ox,oy,oz), (ox,oy,0), 1,0,0)

    # --- Inner faces (5 faces, top open) ---
    # Inner bottom (+Z, floor top surface)
    stl.add_tri((w,w,w), (w+w,iy,w), (w+w,w,w), 0,0,1)
    stl.add_tri((w,w,w), (w,iy,w), (w+w,iy,w), 0,0,1)
    # Inner front (-Y)
    stl.add_tri((w,iy,w), (w+w,iy,w), (w+w,iy,iz), 0,-1,0)
    stl.add_tri((w,iy,w), (w+w,iy,iz), (w,iy,iz), 0,-1,0)
    # Inner back (+Y)
    stl.add_tri((w,w,w), (w,w,iz), (w+w,w,iz), 0,1,0)
    stl.add_tri((w,w,w), (w+w,w,iz), (w+w,w,w), 0,1,0)
    # Inner left (+X)
    stl.add_tri((w,w,w), (w,iy,w), (w,iy,iz), 1,0,0)
    stl.add_tri((w,w,w), (w,iy,iz), (w,w,iz), 1,0,0)
    # Inner right (-X)
    stl.add_tri((w+w,w,w), (w+w,w,iz), (w+w,iy,iz), -1,0,0)
    stl.add_tri((w+w,w,w), (w+w,iy,iz), (w+w,iy,w), -1,0,0)

    # --- USB opening on back face (-Y, at x=USB_CX) ---
    ux0 = USB_CX - USB_W / 2
    ux1 = USB_CX + USB_W / 2
    uz0 = 0
    uz1 = USB_H
    # Back face: split into 3 quads around the USB hole
    # Left quad
    stl.add_tri((0,0,0), (ux0,0,0), (ux0,0,uz1), 0,-1,0)
    stl.add_tri((0,0,0), (ux0,0,uz1), (0,0,uz1), 0,-1,0)
    stl.add_tri((0,0,uz1), (ux0,0,uz1), (ux0,0,oz), 0,-1,0)
    stl.add_tri((0,0,uz1), (ux0,0,oz), (0,0,oz), 0,-1,0)
    stl.add_tri((0,0,0), (ux0,0,0), (ux0,0,0), 0,0,-1)  # bottom strip
    # Right quad
    stl.add_tri((ux1,0,0), (ox,0,0), (ox,0,uz1), 0,-1,0)
    stl.add_tri((ux1,0,0), (ox,0,uz1), (ux1,0,uz1), 0,-1,0)
    stl.add_tri((ux1,0,uz1), (ox,0,uz1), (ox,0,oz), 0,-1,0)
    stl.add_tri((ux1,0,uz1), (ox,0,oz), (ux1,0,oz), 0,-1,0)
    # Top quad
    stl.add_tri((ux0,0,uz1), (ux1,0,uz1), (ux1,0,oz), 0,-1,0)
    stl.add_tri((ux0,0,uz1), (ux1,0,oz), (ux0,0,oz), 0,-1,0)
    # Bottom quad
    stl.add_tri((ux0,0,0), (ux1,0,0), (ux1,0,uz1), 0,-1,0)
    stl.add_tri((ux0,0,0), (ux1,0,uz1), (ux0,0,uz1), 0,-1,0)
    # Inner wall of USB hole
    stl.add_tri((ux0,0,0), (ux0,0,uz1), (ux0,w,uz1), 0,1,0)
    stl.add_tri((ux0,0,0), (ux0,w,uz1), (ux0,w,0), 0,1,0)
    stl.add_tri((ux1,0,0), (ux1,w,0), (ux1,w,uz1), 0,-1,0)
    stl.add_tri((ux1,0,0), (ux1,w,uz1), (ux1,0,uz1), 0,-1,0)
    stl.add_tri((ux0,0,uz1), (ux1,0,uz1), (ux1,w,uz1), 0,0,-1)
    stl.add_tri((ux0,0,uz1), (ux1,w,uz1), (ux0,w,uz1), 0,0,-1)

    # --- Vent hole on bottom face (-Z, cylinder approximation) ---
    add_cylinder_hole_z(stl, VENT_CX, VENT_CY, 0, VENT_D / 2, w, 12)

    # --- Buzzer holes on right side (+X face) ---
    for i in range(N_HOLES):
        bx = ox
        by = BUZZER_START_X + i * 8.0
        bz = OUTER_Z / 2
        add_cylinder_hole_x(stl, bx, by, bz, BUZZER_D / 2, w, 8)

    stl.write("bottom_shell.stl")


def add_cylinder_hole_z(stl, cx, cy, z, r, depth, n_seg=12):
    """Hole through bottom face at (cx,cy) from z=0 to z=depth.
    Creates inner wall + caps."""
    import math
    for i in range(n_seg):
        a0 = 2 * math.pi * i / n_seg
        a1 = 2 * math.pi * (i + 1) / n_seg
        x0 = cx + r * math.cos(a0)
        y0 = cy + r * math.sin(a0)
        x1 = cx + r * math.cos(a1)
        y1 = cy + r * math.sin(a1)

        # Inner wall (facing outward from center)
        nx = math.cos((a0 + a1) / 2)
        ny = math.sin((a0 + a1) / 2)
        stl.add_tri((x0,y0,0), (x0,y0,depth), (x1,y1,depth), nx,ny,0)
        stl.add_tri((x0,y0,0), (x1,y1,depth), (x1,y1,0), nx,ny,0)

    # Bottom cap (z=0, facing -Z)
    for i in range(n_seg):
        a0 = 2 * math.pi * i / n_seg
        a1 = 2 * math.pi * (i + 1) / n_seg
        x0 = cx + r * math.cos(a0)
        y0 = cy + r * math.sin(a0)
        x1 = cx + r * math.cos(a1)
        y1 = cy + r * math.sin(a1)
        stl.add_tri((cx,cy,0), (x1,y1,0), (x0,y0,0), 0,0,-1)

    # Top cap (z=depth, facing +Z)
    for i in range(n_seg):
        a0 = 2 * math.pi * i / n_seg
        a1 = 2 * math.pi * (i + 1) / n_seg
        x0 = cx + r * math.cos(a0)
        y0 = cy + r * math.sin(a0)
        x1 = cx + r * math.cos(a1)
        y1 = cy + r * math.sin(a1)
        stl.add_tri((cx,cy,depth), (x0,y0,depth), (x1,y1,depth), 0,0,1)


def add_cylinder_hole_x(stl, x, cy, cz, r, depth, n_seg=8):
    """Hole through +X face at (cy,cz), extending inward by depth."""
    import math
    for i in range(n_seg):
        a0 = 2 * math.pi * i / n_seg
        a1 = 2 * math.pi * (i + 1) / n_seg
        y0 = cy + r * math.cos(a0)
        z0 = cz + r * math.sin(a0)
        y1 = cy + r * math.cos(a1)
        z1 = cz + r * math.sin(a1)

        # Inner wall
        ny = math.cos((a0 + a1) / 2)
        nz = math.sin((a0 + a1) / 2)
        stl.add_tri((x,y0,z0), (x-depth,y0,z0), (x-depth,y1,z1), 1,0,0)
        stl.add_tri((x,y0,z0), (x-depth,y1,z1), (x,y1,z1), 1,0,0)

    # Outer cap (x=x, facing +X)
    for i in range(n_seg):
        a0 = 2 * math.pi * i / n_seg
        a1 = 2 * math.pi * (i + 1) / n_seg
        y0 = cy + r * math.cos(a0)
        z0 = cz + r * math.sin(a0)
        y1 = cy + r * math.cos(a1)
        z1 = cz + r * math.sin(a1)
        stl.add_tri((x,cy,cz), (x,y0,z0), (x,y1,z1), 1,0,0)

    # Inner cap (x=x-depth, facing -X)
    for i in range(n_seg):
        a0 = 2 * math.pi * i / n_seg
        a1 = 2 * math.pi * (i + 1) / n_seg
        y0 = cy + r * math.cos(a0)
        z0 = cz + r * math.sin(a0)
        y1 = cy + r * math.cos(a1)
        z1 = cz + r * math.sin(a1)
        stl.add_tri((x-depth,cy,cz), (x-depth,y1,z1), (x-depth,y0,z0), -1,0,0)


if __name__ == "__main__":
    print("Generating STL files...")
    generate_front_panel()
    generate_bottom_shell()
    print("Done!")
