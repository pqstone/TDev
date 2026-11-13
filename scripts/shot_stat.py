import os
from PIL import Image
p = os.path.join(os.path.dirname(os.path.abspath(__file__)), "shot_main.png")
im = Image.open(p).convert("RGB")
w, h = im.size
px = im.load()
n = 0
step = 4
tot = 0
for y in range(0, h, step):
    for x in range(0, w, step):
        r, g, b = px[x, y]
        tot += 1
        if not (r > 245 and g > 245 and b > 245):
            n += 1
print("size", w, h, "sampled", tot, "non_white_ratio", round(n / tot * 100, 2), "%")
# 左侧侧边栏区域（前 220px）是否也有内容
n2 = t2 = 0
for y in range(0, h, step):
    for x in range(0, min(220, w), step):
        r, g, b = px[x, y]
        t2 += 1
        if not (r > 245 and g > 245 and b > 245):
            n2 += 1
print("sidebar_non_white_ratio", round(n2 / t2 * 100, 2), "%")
