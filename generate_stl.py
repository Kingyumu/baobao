#!/usr/bin/env python3
"""Generate correct ASCII STL files for the weather station enclosure.

Output:
  front_panel.stl  — 前面板：5mm厚平板 + 屏幕窗口 + 触摸区薄壁
  bottom_shell.stl — 底壳：中空盒子（顶面开口）+ USB/透气/蜂鸣器孔

Units: millimeters.  Run: python generate_stl.py
"""

import math

# === Dimensions (mm) ===
OUTER_X, OUTER_Y = 100.0, 80.0
WALL = 2.0

# Front panel
PANEL_THICK = 5.0
SCREEN_W, SCREEN_H = 75.0, 51.0
SCREEN_CX, SCREEN_CY = OUTER_X / 2, OUTER_Y / 2 + 5

# Bottom shell (open top)
SHELL_HEIGHT = 35.0
SHELL_DEPTH = SHELL_HEIGHT - PANEL_THICK  # 30mm usable inside

# Cutouts on bottom shell
USB_W, USB_H = 9.0, 4.0
USB_CX = OUTER_X / 2
VENT_D = 6.0
VENT_CX, VENT_CY = 25.0, OUTER_Y / 2
BUZZER_D = 4.0
BUZZER_CY = OUTER_Y / 2
N_BUZZER = 3


class StlWriter:
    def __init__(self, name):
        self.name = name
        self.tris = []

    def tri(self, v1, v2, v3, n=None):
        if n is None:
            # Auto-calculate face normal
            ax, ay, az = v2[0]-v1[0], v2[1]-v1[1], v2[2]-v1[2]
            bx, by, bz = v3[0]-v1[0], v3[1]-v1[1], v3[2]-v1[2]
            nx = ay*bz - az*by
            ny = az*bx - ax*bz
            nz = ax*by - ay*bx
            ln = math.sqrt(nx*nx + ny*ny + nz*nz) or 1.0
            n = (nx/ln, ny/ln, nz/ln)
        self.tris.append((v1, v2, v3, n))

    def quad(self, v1, v2, v3, v4, n=None):
        """Two triangles for a quad (v1-v2-v3-v4, counter-clockwise)."""
        self.tri(v1, v2, v3, n)
        self.tri(v1, v3, v4, n)

    def write(self, path):
        with open(path, 'w') as f:
            f.write(f"solid {self.name}\n")
            for v1, v2, v3, n in self.tris:
                f.write(f"  facet normal {n[0]:.6f} {n[1]:.6f} {n[2]:.6f}\n")
                f.write(f"    outer loop\n")
                f.write(f"      vertex {v1[0]:.4f} {v1[1]:.4f} {v1[2]:.4f}\n")
                f.write(f"      vertex {v2[0]:.4f} {v2[1]:.4f} {v2[2]:.4f}\n")
                f.write(f"      vertex {v3[0]:.4f} {v3[1]:.4f} {v3[2]:.4f}\n")
                f.write(f"    endloop\n")
                f.write(f"  endfacet\n")
            f.write(f"endsolid {self.name}\n")
        print(f"  → {path} ({len(self.tris)} triangles)")


def rect_ring_quads(s, z, x0, y0, x1, y1, hx0, hy0, hx1, hy1, n):
    """Rectangular ring on z-plane, split into 4 quads (each 2 triangles).
    Ring outer: (x0,y0)-(x1,y1), inner hole: (hx0,hy0)-(hx1,hy1)."""
    # Bottom strip
    s.quad((x0,y0,z),(hx0,y0,z),(hx0,hy0,z),(x0,hy0,z), n)
    # Top strip
    s.quad((x0,hy1,z),(hx0,hy1,z),(hx1,hy1,z),(x1,hy1,z), n)  # wrong order
    # Actually let me think about this more carefully.
    # The ring on z-plane has 4 strips:
    # Bottom: y=y0, x: x0→hx0 and hx1→x1
    # Top:    y=y1, x: x0→hx0 and hx1→x1
    # Left:   x=x0, y: y0→hy0 and hy1→y1
    # Right:  x=x1, y: y0→hy0 and hy1→y1

    # Bottom-left
    s.quad((x0,y0,z), (hx0,y0,z), (hx0,hy0,z), (x0,hy0,z), n)
    # Bottom-right
    s.quad((hx1,y0,z), (x1,y0,z), (x1,hy0,z), (hx1,hy0,z), n)
    # Top-left
    s.quad((x0,hy1,z), (hx0,hy1,z), (hx0,y1,z), (x0,y1,z), n)
    # Top-right
    s.quad((hx1,hy1,z), (x1,hy1,z), (x1,y1,z), (hx1,y1,z), n)


def box_shell(s, x0, y0, z0, x1, y1, z1, n_out=-1):
    """Solid box 6 faces, each 2 triangles. n_out: -1 for inward normals (inside view)."""
    sign = 1 if n_out == 1 else -1
    # Bottom (-Z)
    s.tri((x0,y0,z0),(x1,y1,z0),(x1,y0,z0), (0,0,-1*sign))
    s.tri((x0,y0,z0),(x0,y1,z0),(x1,y1,z0), (0,0,-1*sign))
    # Top (+Z)
    s.tri((x0,y0,z1),(x1,y0,z1),(x1,y1,z1), (0,0,1*sign))
    s.tri((x0,y0,z1),(x1,y1,z1),(x0,y1,z1), (0,0,1*sign))
    # Front (+Y)
    s.tri((x0,y1,z0),(x1,y1,z1),(x1,y1,z0), (0,1,0)* (1,))
    s.tri((x0,y1,z0),(x0,y1,z1),(x1,y1,z1), (0,1,0))
    # Back (-Y)
    s.tri((x0,y0,z0),(x1,y0,z0),(x1,y0,z1), (0,-1,0))
    s.tri((x0,y0,z0),(x1,y0,z1),(x0,y0,z1), (0,-1,0))
    # Left (-X)
    s.tri((x0,y0,z0),(x0,y1,z0),(x0,y1,z1), (-1,0,0))
    s.tri((x0,y0,z0),(x0,y1,z1),(x0,y0,z1), (-1,0,0))
    # Right (+X)
    s.tri((x1,y0,z0),(x1,y0,z1),(x1,y1,z1), (1,0,0))
    s.tri((x1,y0,z0),(x1,y1,z1),(x1,y1,z0), (1,0,0))


def cylinder_wall(s, cx, cy, z0, z1, r, n_seg=16):
    """Vertical cylinder wall (open top and bottom)."""
    for i in range(n_seg):
        a0 = 2*math.pi*i/n_seg
        a1 = 2*math.pi*(i+1)/n_seg
        x0 = cx + r*math.cos(a0)
        y0 = cy + r*math.sin(a0)
        x1 = cx + r*math.cos(a1)
        y1 = cy + r*math.sin(a1)
        s.quad((x0,y0,z0),(x1,y0,z0),(x1,y0,z1),(x0,y0,z1),
               (math.cos((a0+a1)/2), math.sin((a0+a1)/2), 0))


# =============================================
# 1. FRONT PANEL — flat slab with screen window
# =============================================
def gen_front_panel():
    s = StlWriter("front_panel")
    T = PANEL_THICK
    sx0 = SCREEN_CX - SCREEN_W/2
    sy0 = SCREEN_CY - SCREEN_H/2
    sx1 = SCREEN_CX + SCREEN_W/2
    sy1 = SCREEN_CY + SCREEN_H/2

    # Top face (z=T): ring with screen hole
    rect_ring_quads(s, T, 0,0,OUTER_X,OUTER_Y, sx0,sy0,sx1,sy1, (0,0,1))
    # Bottom face (z=0): ring with screen hole (reversed winding)
    rect_ring_quads(s, 0, 0,0,OUTER_X,OUTER_Y, sx0,sy0,sx1,sy1, (0,0,-1))
    # Fix: bottom face normals should be (0,0,-1), ring_quads already sets n

    # Side walls (4 faces, outer)
    # Front (+Y)
    s.quad((0,OUTER_Y,0),(OUTER_X,OUTER_Y,0),(OUTER_X,OUTER_Y,T),(0,OUTER_Y,T), (0,1,0))
    # Back (-Y)
    s.quad((0,0,0),(0,0,T),(OUTER_X,0,T),(OUTER_X,0,0), (0,-1,0))
    # Left (-X)
    s.quad((0,0,0),(0,OUTER_Y,0),(0,OUTER_Y,T),(0,0,T), (-1,0,0))
    # Right (+X)
    s.quad((OUTER_X,0,0),(OUTER_X,0,T),(OUTER_X,OUTER_Y,T),(OUTER_X,OUTER_Y,0), (1,0,0))

    # Screen window inner walls (4 strips, T tall)
    # -Y inner (front side of window)
    s.quad((sx0,sy0,0),(sx1,sy0,0),(sx1,sy0,T),(sx0,sy0,T), (0,1,0))
    # +Y inner (back side)
    s.quad((sx0,sy1,0),(sx0,sy1,T),(sx1,sy1,T),(sx1,sy1,0), (0,-1,0))
    # -X inner (left side)
    s.quad((sx0,sy0,0),(sx0,sy0,T),(sx0,sy1,T),(sx0,sy1,0), (1,0,0))
    # +X inner (right side)
    s.quad((sx1,sy0,0),(sx1,sy1,0),(sx1,sy1,T),(sx1,sy0,T), (-1,0,0))

    s.write("front_panel.stl")


# =============================================
# 2. BOTTOM SHELL — hollow box, open top, with cutouts
# =============================================
def gen_bottom_shell():
    s = StlWriter("bottom_shell")
    h = SHELL_HEIGHT
    ux0 = USB_CX - USB_W/2
    ux1 = USB_CX + USB_W/2
    uz1 = USB_H

    # --- Outer walls (4 sides, no top, bottom intact) ---
    # Bottom (-Z): full solid
    s.tri((0,0,0),(OUTER_X,OUTER_Y,0),(OUTER_X,0,0), (0,0,-1))
    s.tri((0,0,0),(0,OUTER_Y,0),(OUTER_X,OUTER_Y,0), (0,0,-1))
    # Front (+Y)
    s.quad((0,OUTER_Y,0),(OUTER_X,OUTER_Y,0),(OUTER_X,OUTER_Y,h),(0,OUTER_Y,h), (0,1,0))
    # Left (-X)
    s.quad((0,0,0),(0,OUTER_Y,0),(0,OUTER_Y,h),(0,0,h), (-1,0,0))
    # Right (+X)
    s.quad((OUTER_X,0,0),(OUTER_X,0,h),(OUTER_X,OUTER_Y,h),(OUTER_X,OUTER_Y,0), (1,0,0))
    # Back (-Y): USB cutout
    s.quad((0,0,0),(ux0,0,0),(ux0,0,uz1),(0,0,uz1), (0,-1,0))
    s.quad((0,0,uz1),(ux0,0,uz1),(ux0,0,h),(0,0,h), (0,-1,0))
    s.quad((ux1,0,0),(OUTER_X,0,0),(OUTER_X,0,uz1),(ux1,0,uz1), (0,-1,0))
    s.quad((ux1,0,uz1),(OUTER_X,0,uz1),(OUTER_X,0,h),(ux1,0,h), (0,-1,0))
    s.quad((ux0,0,uz1),(ux1,0,uz1),(ux1,0,h),(ux0,0,h), (0,-1,0))

    # --- Inner walls ---
    w = WALL
    # Inner back (+Y, inside face)
    s.quad((w,w,w),(w+w,w,w),(w+w,w,h),(w,w,h), (0,1,0))
    # Inner front (-Y, inside face)
    s.quad((w,OUTER_Y-w,w),(OUTER_X-w,OUTER_Y-w,w),(OUTER_X-w,OUTER_Y-w,h),(w,OUTER_Y-w,h), (0,-1,0))
    # Inner left (+X, inside face)
    s.quad((w,w,w),(w,w,h),(w,OUTER_Y-w,h),(w,OUTER_Y-w,w), (1,0,0))
    # Inner right (-X, inside face)
    s.quad((OUTER_X-w,w,w),(OUTER_X-w,OUTER_Y-w,w),(OUTER_X-w,OUTER_Y-w,h),(OUTER_X-w,w,h), (-1,0,0))

    # USB hole inner walls
    s.quad((ux0,0,w),(ux0,w,w),(ux0,w,uz1),(ux0,0,uz1), (1,0,0))
    s.quad((ux1,0,w),(ux1,0,uz1),(ux1,w,uz1),(ux1,w,w), (-1,0,0))
    s.quad((ux0,0,uz1),(ux1,0,uz1),(ux1,w,uz1),(ux0,w,uz1), (0,0,-1))

    # Vent hole cylinder (through bottom, z=0 to z=w)
    cylinder_wall(s, VENT_CX, VENT_CY, 0, w, VENT_D/2)
    # Vent bottom cap
    for i in range(16):
        a0 = 2*math.pi*i/16
        a1 = 2*math.pi*(i+1)/16
        s.tri((VENT_CX,VENT_CY,0),
              (VENT_CX+VENT_D/2*math.cos(a1),VENT_CY+VENT_D/2*math.sin(a1),0),
              (VENT_CX+VENT_D/2*math.cos(a0),VENT_CY+VENT_D/2*math.sin(a0),0),
              (0,0,-1))

    # Buzzer holes on right side (+X face)
    for i in range(N_BUZZER):
        bx = OUTER_X
        by = 20.0 + i * 20.0
        bz = h / 2
        cylinder_wall(s, bx, by, bz-BUZZER_D/2, bz+BUZZER_D/2, BUZZER_D/2, 8)
        # Outer cap
        for j in range(8):
            a0 = 2*math.pi*j/8
            a1 = 2*math.pi*(j+1)/8
            s.tri((bx, by+BUZZER_D/2*math.cos(a0), bz+BUZZER_D/2*math.sin(a0)),
                  (bx, by+BUZZER_D/2*math.cos(a1), bz+BUZZER_D/2*math.sin(a1)),
                  (bx, by, bz),
                  (1,0,0))

    s.write("bottom_shell.stl")


if __name__ == "__main__":
    print("Generating STL files...")
    gen_front_panel()
    gen_bottom_shell()
    print("Done!")
