-- Copy this directory and replace the atlas data. No C++ subclass or main-loop
-- change is required for a new species.
return {
    api_version = 1,
    id = "template",
    name = "Template Bug",
    behavior = "behavior.lua",
    atlas = {
        file = "atlas.png",
        width = 283,
        height = 799,
        reference_length = 799,
    },
    body = {
        default_length = 120,
        overlay_scale = 2.0,
        collider_half_width = 0.25,
        collider_half_length = 0.40,
        root_part = "body",
    },
    capabilities = {
        bait = false,
    },
    parts = {
        {
            name = "body",
            source = { 0, 0, 283, 799 },
            pivot = { 141.5, 399.5 },
            attachment = { 0.0, 0.0 },
            layer = 0,
        },
    },
}
