return {
    api_version = 1,
    id = "cockroach",
    name = "Cockroach",
    behavior = "behavior.lua",

    atlas = {
        file = "cockroach_parts_atlas.png",
        width = 1536,
        height = 1024,
        reference_length = 799,
    },

    body = {
        default_length = 165,
        overlay_scale = 2.15,
        collider_half_width = 0.20,
        collider_half_length = 0.43,
        root_part = "body",
    },

    capabilities = {
        bait = true,
    },

    render = {
        color = { 190, 190, 190, 255 },
        shadow = {
            color = { 0, 0, 0, 38 },
            offset = { 3, 5 },
        },
    },

    parts = {
        {
            name = "body",
            source = { 0, 0, 283, 799 },
            pivot = { 141.5, 399.5 },
            attachment = { 0.0, 0.0 },
            layer = 100,
        },
        {
            name = "left_front_leg",
            source = { 284, 248, 301, 273 },
            pivot = { 286.0, 89.0 },
            attachment = { -0.155, -0.305 },
            layer = 10,
        },
        {
            name = "right_front_leg",
            source = { 585, 248, 304, 273 },
            pivot = { 14.0, 89.0 },
            attachment = { 0.155, -0.305 },
            layer = 11,
        },
        {
            name = "left_middle_leg",
            source = { 889, 248, 280, 350 },
            pivot = { 266.0, 35.0 },
            attachment = { -0.170, -0.075 },
            layer = 12,
        },
        {
            name = "right_middle_leg",
            source = { 1169, 248, 286, 348 },
            pivot = { 17.0, 40.0 },
            attachment = { 0.170, -0.075 },
            layer = 13,
        },
        {
            name = "left_rear_leg",
            source = { 284, 598, 219, 313 },
            pivot = { 208.0, 14.0 },
            attachment = { -0.150, 0.180 },
            layer = 14,
        },
        {
            name = "right_rear_leg",
            source = { 503, 598, 218, 312 },
            pivot = { 11.0, 14.0 },
            attachment = { 0.150, 0.180 },
            layer = 15,
        },
        {
            name = "left_antenna",
            source = { 284, 0, 526, 248 },
            pivot = { 517.0, 237.0 },
            attachment = { -0.070, -0.430 },
            layer = 20,
        },
        {
            name = "right_antenna",
            source = { 810, 0, 531, 248 },
            pivot = { 10.0, 237.0 },
            attachment = { 0.070, -0.430 },
            layer = 21,
        },
    },
}
