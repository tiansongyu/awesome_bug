return {
    api_version = 1,
    id = "turtle",
    name = "Little Tortoise",
    behavior = "behavior.lua",

    atlas = {
        file = "turtle_parts_atlas.png",
        width = 1536,
        height = 1024,
        reference_length = 1066,
    },

    body = {
        default_length = 118,
        overlay_scale = 1.85,
        collider_half_width = 0.285,
        collider_half_length = 0.335,
        root_part = "body",
    },

    capabilities = {
        bait = true,
    },

    render = {
        color = { 232, 232, 232, 255 },
        bait = "lettuce",
        shadow = {
            color = { 0, 0, 0, 30 },
            offset = { 2, 3 },
        },
    },

    parts = {
        {
            name = "body",
            source = { 0, 0, 700, 750 },
            pivot = { 350, 370 },
            attachment = { 0.0, 0.0 },
            layer = 100,
        },
        {
            name = "head",
            source = { 720, 0, 230, 238 },
            pivot = { 115, 238 },
            -- Keep the neck root beneath the shell so head turns cannot open
            -- a transparent seam between the two sprites.
            attachment = { 0.0, -0.315 },
            layer = 20,
        },
        {
            name = "left_front_leg",
            source = { 720, 260, 205, 180 },
            pivot = { 145, 105 },
            attachment = { -0.145, -0.250 },
            layer = 10,
        },
        {
            name = "right_front_leg",
            source = { 940, 260, 205, 180 },
            pivot = { 60, 105 },
            attachment = { 0.145, -0.250 },
            layer = 11,
        },
        {
            name = "left_rear_leg",
            source = { 720, 460, 180, 180 },
            pivot = { 105, 65 },
            attachment = { -0.182, 0.228 },
            layer = 12,
        },
        {
            name = "right_rear_leg",
            source = { 920, 460, 180, 180 },
            pivot = { 75, 65 },
            attachment = { 0.182, 0.228 },
            layer = 13,
        },
        {
            name = "tail",
            source = { 1120, 460, 74, 118 },
            pivot = { 37, 0 },
            attachment = { 0.0, 0.315 },
            layer = 14,
        },
    },
}
