#!/usr/bin/env python3
"""Extract the nine disconnected cockroach sprites from the generated atlas."""

from pathlib import Path
import sys

import cv2
import numpy as np
from PIL import Image


PART_NAMES = (
    "body",
    "left_front_leg",
    "right_front_leg",
    "left_middle_leg",
    "right_middle_leg",
    "left_rear_leg",
    "right_rear_leg",
    "left_antenna",
    "right_antenna",
)

ATLAS_SIZE = (1536, 1024)
ATLAS_POSITIONS = {
    "body": (0, 0),
    "left_antenna": (284, 0),
    "right_antenna": (810, 0),
    "left_front_leg": (284, 248),
    "right_front_leg": (585, 248),
    "left_middle_leg": (889, 248),
    "right_middle_leg": (1169, 248),
    "left_rear_leg": (284, 598),
    "right_rear_leg": (503, 598),
}


def classify(components):
    body = max(components, key=lambda item: item["area"])
    remaining = [item for item in components if item is not body]
    antennae = sorted(
        sorted(remaining, key=lambda item: item["cy"])[:2],
        key=lambda item: item["cx"],
    )
    legs = [item for item in remaining if item not in antennae]
    left_legs = sorted(
        (item for item in legs if item["cx"] < body["cx"]),
        key=lambda item: item["cy"],
    )
    right_legs = sorted(
        (item for item in legs if item["cx"] > body["cx"]),
        key=lambda item: item["cy"],
    )
    if len(left_legs) != 3 or len(right_legs) != 3:
        raise RuntimeError("expected three legs on each side")
    return {
        "body": body,
        "left_front_leg": left_legs[0],
        "right_front_leg": right_legs[0],
        "left_middle_leg": left_legs[1],
        "right_middle_leg": right_legs[1],
        "left_rear_leg": left_legs[2],
        "right_rear_leg": right_legs[2],
        "left_antenna": antennae[0],
        "right_antenna": antennae[1],
    }


def main():
    source = (
        Path(sys.argv[1])
        if len(sys.argv) > 1
        else Path("assets/cockroach_parts_sheet.png")
    )
    output = (
        Path(sys.argv[2])
        if len(sys.argv) > 2
        else Path("assets/cockroach_parts")
    )
    image = Image.open(source).convert("RGBA")
    pixels = np.asarray(image)
    count, labels, stats, centroids = cv2.connectedComponentsWithStats(
        (pixels[:, :, 3] > 8).astype(np.uint8), connectivity=8
    )
    components = []
    for index in range(1, count):
        x, y, width, height, area = (int(value) for value in stats[index])
        if area < 1000:
            continue
        components.append(
            {
                "label": index,
                "x": x,
                "y": y,
                "width": width,
                "height": height,
                "area": area,
                "cx": float(centroids[index][0]),
                "cy": float(centroids[index][1]),
            }
        )
    if len(components) != len(PART_NAMES):
        raise RuntimeError(
            f"expected {len(PART_NAMES)} components, found {len(components)}"
        )

    output.mkdir(parents=True, exist_ok=True)
    atlas = Image.new("RGBA", ATLAS_SIZE, (0, 0, 0, 0))
    for name, component in classify(components).items():
        x = component["x"]
        y = component["y"]
        width = component["width"]
        height = component["height"]
        isolated = np.zeros((height, width, 4), dtype=np.uint8)
        region = pixels[y : y + height, x : x + width]
        mask = labels[y : y + height, x : x + width] == component["label"]
        isolated[mask] = region[mask]
        part_image = Image.fromarray(isolated, "RGBA")
        part_image.save(output / f"{name}.png")
        atlas.alpha_composite(part_image, ATLAS_POSITIONS[name])
        print(f"{name}: x={x} y={y} w={width} h={height}")
    atlas_path = output.parent / "cockroach_parts_atlas.png"
    atlas.save(atlas_path)
    print(f"atlas: {atlas_path} ({ATLAS_SIZE[0]}x{ATLAS_SIZE[1]})")


if __name__ == "__main__":
    main()
