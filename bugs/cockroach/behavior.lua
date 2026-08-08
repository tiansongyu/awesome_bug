-- Cockroach policy and animation controller.
--
-- ABI v1 is deliberately table-only:
--   module.new(config, host) -> controller
--   controller:step(frame)   -> state/target/motion/events
--   controller:pose(frame)   -> body and named-part transforms
--
-- The controller owns every species-specific mutable value. The host owns
-- Windows input, collision constraints and integration. `host.random` is the
-- only source of entropy, which keeps one and twenty instance modes on the
-- same deterministic code path. `host.fsm` is the shared, sandbox-loaded
-- runtime module; species scripts never use package/require.

local pi = math.pi
local deg = pi / 180.0

local function vec(x, y)
    return { x = x or 0.0, y = y or 0.0 }
end

local function xy(value)
    if type(value) ~= "table" then
        return 0.0, 0.0
    end
    return value.x or value[1] or 0.0, value.y or value[2] or 0.0
end

local function add(a, b)
    local ax, ay = xy(a)
    local bx, by = xy(b)
    return vec(ax + bx, ay + by)
end

local function sub(a, b)
    local ax, ay = xy(a)
    local bx, by = xy(b)
    return vec(ax - bx, ay - by)
end

local function mul(value, scale)
    local x, y = xy(value)
    return vec(x * scale, y * scale)
end

local function dot(a, b)
    local ax, ay = xy(a)
    local bx, by = xy(b)
    return ax * bx + ay * by
end

local function length(value)
    local x, y = xy(value)
    return math.sqrt(x * x + y * y)
end

local function normalized(value)
    local magnitude = length(value)
    if magnitude < 0.000001 then
        return vec()
    end
    return mul(value, 1.0 / magnitude)
end

local function position_of(value)
    return vec(value.x, value.y)
end

local function velocity_of(value)
    return vec(value.vx, value.vy)
end

local function world_rect(frame)
    return frame.world
end

local function body_position(frame)
    return position_of(frame.body)
end

local function body_length(frame)
    return frame.body.length
end

local function feature_enabled(frame, name)
    return frame.features[name] == true
end

local function forward(heading)
    return vec(math.sin(heading), -math.cos(heading))
end

local function angle_of(direction)
    local x, y = xy(direction)
    return math.atan(x, -y)
end

local function finite_or(value, fallback)
    if type(value) ~= "number" or value ~= value
        or value == math.huge or value == -math.huge then
        return fallback
    end
    return value
end

local function new_controller(config, host)
    config = config or {}

    local self = {
        initialized = false,
        fsm = nil,
        state_timer = 0.0,
        state_clock = 0.0,
        behavior_clock = 0.0,
        gait_clock = 0.0,
        threat_cooldown = 0.0,
        threat_latched = false,
        shelter_timer = 0.0,
        food_retry_timer = 0.0,
        groomed_during_rest = false,
        target = vec(),
        pending_flee_direction = vec(),
        feeding_bait_position = vec(),
        steering_phase = 0.0,
        speed_pulse_phase = 0.0,
        desired_speed = 0.0,
        initial_heading = nil,
        recovery_timer = 0.0,
        recovery_direction = vec(),
        recovery_was_active = false,
        stuck_escape_active = false,
        stuck_escape_direction = vec(),
        stuck_escape_distance = 0.0,
        stuck_escape_speed = 0.0,
        stuck_escape_clear_timer = 0.0,
        stuck_escape_repath_timer = 0.0,
    }

    local function random(tag, low, high)
        return host.random(tag, low, high)
    end

    -- The Rust host uses stock Lua 5.4 numbers (double). Quantize persistent
    -- controller state at the historical float boundaries so deterministic
    -- seeds continue to produce the frozen v1 behavior.
    local function f32(value)
        return host.f32(value)
    end

    local function f32_vec(value)
        local x, y = xy(value)
        return vec(f32(x), f32(y))
    end

    local function clamp(value, low, high)
        return host.clamp(value, low, high)
    end

    local function wrap_angle(value)
        return host.wrap_angle(value)
    end

    local function choose_wander_target(frame)
        local world = world_rect(frame)
        local body = body_length(frame)
        -- These values become tagged-RNG range bounds. Reproduce the old
        -- float-ABI operation boundaries explicitly so the recorded C++ tape
        -- remains bit-exact under stock Lua 5.4's double ABI.
        local half_length = f32(body * f32(0.43))
        local half_width = f32(body * f32(0.20))
        local safe_extent = f32(math.sqrt(f32(
            f32(half_length * half_length)
                + f32(half_width * half_width))))
        local margin = f32(safe_extent + f32(18.0))
        local margin_x = math.min(
            margin, f32(world.width * f32(0.45)))
        local margin_y = math.min(
            margin, f32(world.height * f32(0.45)))
        local low_x = f32(world.x + margin_x)
        local high_x = f32(
            f32(world.x + world.width) - margin_x)
        local low_y = f32(world.y + margin_y)
        local high_y = f32(
            f32(world.y + world.height) - margin_y)
        self.target = f32_vec(vec(
            random("wander.target.x",
                low_x, high_x),
            random("wander.target.y",
                low_y, high_y)))
    end

    local function computed_corners(frame)
        local world = world_rect(frame)
        local body = body_length(frame)
        local half_length = body * 0.43
        local half_width = body * 0.20
        local margin = math.sqrt(
            half_length * half_length + half_width * half_width) + 12.0
        return {
            { x = world.x + margin, y = world.y + margin, blocked = false },
            {
                x = world.x + world.width - margin,
                y = world.y + margin,
                blocked = false,
            },
            {
                x = world.x + margin,
                y = world.y + world.height - margin,
                blocked = false,
            },
            {
                x = world.x + world.width - margin,
                y = world.y + world.height - margin,
                blocked = false,
            },
        }
    end

    local function choose_corner_target(frame)
        local corners = frame.corners
        if type(corners) ~= "table" or #corners < 4 then
            corners = computed_corners(frame)
        end

        local first = math.floor(random("corner.first", 0.0, 3.999)) + 1
        local world = world_rect(frame)
        local position = body_position(frame)
        local best_score = 1.0e9
        local best = position_of(corners[first])

        for offset = 0, 3 do
            local index = ((first - 1 + offset) % 4) + 1
            local corner = corners[index]
            local candidate = position_of(corner)
            local score = length(sub(candidate, position))
                + ((corner.blocked == true)
                    and (world.width + world.height) or 0.0)
                + offset * 0.01
            if score < best_score then
                best_score = score
                best = candidate
            end
        end
        self.target = f32_vec(best)
    end

    if type(host.fsm) ~= "table"
        or type(host.fsm.create) ~= "function" then
        error("host.fsm.create is required by cockroach behavior ABI v1")
    end

    local transition

    local function is(state)
        return self.fsm:is(state)
    end

    local function choose_roaming_behavior(
        frame, pause_threshold, creep_threshold)
        local choice = random("roaming.choice", 0.0, 1.0)
        if choice < pause_threshold then
            transition("pause", frame)
        elseif choice < creep_threshold then
            transition("creep", frame)
        else
            transition("wander", frame)
        end
    end

    local function begin_entry(change)
        self.state_clock = f32(0.0)
        local payload = change.payload or {}
        if type(payload.frame) ~= "table" then
            error("cockroach FSM transition requires a frame")
        end
        return payload.frame, payload.direction
    end

    local function enter_wander(_, change)
        local frame = begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        self.state_timer = f32(random("wander.duration", 0.95, 4.20))
        self.desired_speed =
            f32(random("wander.speed", 112.0, 225.0) * multiplier)
        choose_wander_target(frame)
    end

    local function enter_creep(_, change)
        local frame = begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        self.state_timer = f32(random("creep.duration", 0.85, 2.10))
        self.desired_speed =
            f32(random("creep.speed", 30.0, 62.0) * multiplier)
        choose_wander_target(frame)
    end

    local function enter_pause(_, change)
        begin_entry(change)
        self.state_timer = f32(random("pause.duration", 0.045, 0.24))
        if random("pause.long_roll", 0.0, 1.0) < 0.07 then
            self.state_timer = f32(self.state_timer
                + random("pause.long_duration", 0.25, 0.55))
        end
        self.desired_speed = 0.0
    end

    local function enter_seek_corner(_, change)
        local frame = begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        local position = body_position(frame)
        self.desired_speed =
            f32(random("corner.speed", 48.0, 82.0) * multiplier)
        self.state_timer = f32(clamp(
            length(sub(self.target, position))
                / math.max(40.0, self.desired_speed * 0.62)
                * 1.75 + 2.0,
            12.0, 32.0))
        self.groomed_during_rest = false
    end

    local function enter_lurk(_, change)
        begin_entry(change)
        if self.groomed_during_rest then
            self.state_timer =
                f32(random("lurk.after_groom", 2.0, 3.8))
        else
            self.state_timer =
                f32(random("lurk.before_groom", 4.5, 7.5))
        end
        self.desired_speed = 0.0
    end

    local function enter_groom(_, change)
        begin_entry(change)
        self.state_timer = f32(random("groom.duration", 3.2, 5.2))
        self.desired_speed = 0.0
        self.groomed_during_rest = true
    end

    local function enter_seek_food(_, change)
        begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        self.state_timer =
            f32(random("food.seek_duration", 10.0, 15.0))
        self.desired_speed =
            f32(random("food.seek_speed", 42.0, 70.0) * multiplier)
    end

    local function enter_feeding(_, change)
        begin_entry(change)
        self.state_timer =
            f32(random("food.feed_duration", 2.4, 3.4))
        self.desired_speed = 0.0
    end

    local function enter_startled(_, change)
        local _, direction = begin_entry(change)
        self.state_timer =
            f32(random("threat.startle_duration", 0.055, 0.12))
        self.desired_speed = 0.0
        self.pending_flee_direction =
            f32_vec(normalized(direction or vec()))
    end

    local function enter_flee(_, change)
        local frame, direction = begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        local position = body_position(frame)
        self.state_timer =
            f32(random("threat.flee_duration", 0.72, 1.35))
        self.desired_speed =
            f32(random("threat.flee_speed", 320.0, 450.0) * multiplier)
        local flee_direction = normalized(direction or vec())
        if length(flee_direction) < 0.001 then
            local heading = (frame.body or {}).heading or 0.0
            flee_direction = forward(heading)
        end
        self.target = f32_vec(add(
            position,
            mul(flee_direction,
                random("threat.flee_distance", 380.0, 650.0))))
    end

    local function create_fsm(frame)
        local definition = {
            states = {
                ["pause"] = { enter = enter_pause },
                ["creep"] = { enter = enter_creep },
                ["wander"] = { enter = enter_wander },
                ["seek-corner"] = { enter = enter_seek_corner },
                ["lurk"] = { enter = enter_lurk },
                ["groom"] = { enter = enter_groom },
                ["seek-food"] = { enter = enter_seek_food },
                ["feeding"] = { enter = enter_feeding },
                ["startled"] = { enter = enter_startled },
                ["flee"] = { enter = enter_flee },
            },
            events = {
                to_pause = { from = { "wander" }, to = "pause" },
                to_creep = { from = { "wander" }, to = "creep" },
                to_wander = {
                    from = {
                        "pause", "creep", "wander", "seek-corner",
                        "lurk", "seek-food", "feeding", "flee",
                    },
                    to = "wander",
                },
                ["to_seek-corner"] = {
                    from = {
                        "pause", "creep", "wander", "seek-corner",
                    },
                    to = "seek-corner",
                },
                to_lurk = {
                    from = { "seek-corner", "groom" },
                    to = "lurk",
                },
                to_groom = { from = { "lurk" }, to = "groom" },
                ["to_seek-food"] = {
                    from = {
                        "pause", "creep", "wander", "seek-corner",
                        "lurk", "groom", "feeding",
                    },
                    to = "seek-food",
                },
                to_feeding = {
                    from = { "seek-food" },
                    to = "feeding",
                },
                to_startled = {
                    from = {
                        "pause", "creep", "wander", "seek-corner",
                        "lurk", "groom", "seek-food", "feeding",
                    },
                    to = "startled",
                },
                to_flee = { from = { "startled" }, to = "flee" },
            },
        }
        return host.fsm.create(
            definition, "wander", self, { frame = frame })
    end

    transition = function(state, frame, direction)
        self.fsm:send("to_" .. state, {
            frame = frame,
            direction = direction,
        })
    end

    local function initialize(frame)
        self.initial_heading = f32(random("init.heading", -pi, pi))
        self.behavior_clock =
            f32(random("init.behavior_clock", 0.0, 20.0))
        self.steering_phase =
            f32(random("init.steering_phase", -pi, pi))
        self.speed_pulse_phase =
            f32(random("init.speed_pulse_phase", -pi, pi))
        self.shelter_timer =
            f32(random("init.shelter_timer", 16.0, 34.0))
        self.fsm = create_fsm(frame)
        self.initialized = true
    end

    local function update_behavior(frame)
        local dt = frame.dt
        local extended =
            feature_enabled(frame, "extended_behaviors")
        local position = body_position(frame)
        local body = body_length(frame)
        local cursor = frame.cursor or {}
        local cursor_position = position_of(cursor)
        local cursor_velocity = velocity_of(cursor)

        self.state_timer = f32(self.state_timer - dt)
        self.state_clock = f32(self.state_clock + dt)
        self.threat_cooldown =
            f32(math.max(0.0, self.threat_cooldown - dt))
        self.food_retry_timer =
            f32(math.max(0.0, self.food_retry_timer - dt))

        if extended
            and not is("seek-corner")
            and not is("lurk")
            and not is("groom")
            and not is("seek-food")
            and not is("feeding") then
            self.shelter_timer = f32(self.shelter_timer - dt)
        end

        local cursor_delta = sub(position, cursor_position)
        local cursor_distance = length(cursor_delta)
        local cursor_direction = normalized(cursor_delta)
        local cursor_speed = length(cursor_velocity)
        local approach_speed = dot(cursor_velocity, cursor_direction)
        local rapid_approach =
            cursor_speed >= 250.0 and approach_speed >= 180.0
        local extended_threat =
            cursor_distance < body * 0.82
            or (cursor_distance < body * 2.25 and rapid_approach)
        local proximity_threat = cursor_distance < body * 1.75
        local threat_detected =
            extended and extended_threat
            or (not extended and proximity_threat)
        local cursor_valid = cursor.valid ~= false

        if not cursor_valid or cursor_distance > body * 2.75 then
            self.threat_latched = false
        end
        if cursor_valid and threat_detected
            and not self.threat_latched
            and self.threat_cooldown <= 0.0
            and not is("flee")
            and not is("startled") then
            self.threat_latched = true
            transition("startled", frame, cursor_delta)
        end

        local bait = frame.bait or {}
        local bait_position = position_of(bait)
        local bait_active = bait.active == true
        local can_seek_food =
            is("pause")
            or is("creep")
            or is("wander")
            or is("seek-corner")
            or is("lurk")
            or is("groom")
        if extended and bait_active and can_seek_food
            and self.food_retry_timer <= 0.0 then
            self.target = f32_vec(bait_position)
            transition("seek-food", frame)
        end

        if is("seek-food") then
            if not bait_active then
                self.food_retry_timer = f32(1.5)
                transition("wander", frame)
            else
                self.target = f32_vec(bait_position)
                if length(sub(self.target, position)) < body * 0.34 then
                    self.feeding_bait_position = f32_vec(bait_position)
                    transition("feeding", frame)
                end
            end
        elseif is("feeding") then
            if not bait_active then
                transition("wander", frame)
            elseif length(sub(bait_position, self.feeding_bait_position)) > 2.0
                or length(sub(position, self.feeding_bait_position))
                    > body * 0.52 then
                self.target = f32_vec(bait_position)
                transition("seek-food", frame)
            end
        end

        local can_seek_shelter =
            is("pause")
            or is("creep")
            or is("wander")
        if extended and can_seek_shelter
            and (frame.request_corner_rest == true
                or self.shelter_timer <= 0.0) then
            choose_corner_target(frame)
            transition("seek-corner", frame)
        end

        if is("seek-corner")
            and length(sub(self.target, position)) < body * 0.34 then
            transition("lurk", frame)
        end

        local consume_bait = false
        if self.state_timer <= 0.0 then
            if is("startled") then
                transition("flee", frame, self.pending_flee_direction)
            elseif is("pause")
                or is("flee")
                or is("creep") then
                if is("flee") then
                    self.threat_cooldown =
                        f32(random("threat.cooldown", 0.85, 1.25))
                end
                transition("wander", frame)
            elseif is("seek-corner") then
                choose_corner_target(frame)
                transition("seek-corner", frame)
            elseif is("lurk") then
                if self.groomed_during_rest then
                    self.shelter_timer =
                        f32(random("shelter.retry", 18.0, 38.0))
                    transition("wander", frame)
                else
                    transition("groom", frame)
                end
            elseif is("groom") then
                transition("lurk", frame)
            elseif is("seek-food") then
                self.food_retry_timer =
                    f32(random("food.retry", 2.0, 4.0))
                transition("wander", frame)
            elseif is("feeding") then
                local sensors = frame.sensors or {}
                if sensors.overlapping == true and bait_active then
                    self.target = f32_vec(bait_position)
                    transition("seek-food", frame)
                else
                    consume_bait = true
                    self.shelter_timer =
                        f32(random(
                            "food.shelter_delay", 12.0, 24.0))
                    transition("wander", frame)
                end
            else
                choose_roaming_behavior(frame, 0.18, 0.34)
            end
        end

        if (is("wander") or is("creep"))
            and length(sub(self.target, position)) < body * 0.48 then
            if is("creep") then
                transition("wander", frame)
            else
                choose_roaming_behavior(frame, 0.20, 0.37)
            end
        end

        return consume_bait
    end

    local function update_host_recovery(frame)
        local feedback = frame.feedback

        local dt = finite_or(frame.dt, 0.0)
        local step_dt = math.max(0.0, dt)
        self.recovery_timer = f32(
            math.max(0.0, self.recovery_timer - step_dt))
        self.stuck_escape_repath_timer = f32(math.max(
            0.0, self.stuck_escape_repath_timer - step_dt))

        local clearance =
            finite_or(feedback.recovery_clearance, 0.0)
        local blocked_time = finite_or(feedback.blocked_time, 0.0)
        local edge_dwell_time =
            finite_or(feedback.edge_dwell_time, 0.0)
        local direction =
            normalized(feedback.recovery_direction or vec())
        local actual_distance =
            length(feedback.actual_displacement or vec())
        local stopped_state =
            is("pause")
            or is("lurk")
            or is("groom")
            or is("feeding")
            or is("startled")

        if self.stuck_escape_active then
            if stopped_state then
                self.stuck_escape_active = false
                self.stuck_escape_clear_timer = f32(0.0)
                self.stuck_escape_repath_timer = f32(0.0)
            else
                -- Do not release the detour on a single lucky frame. The
                -- solver's blocked timer decays only after real displacement,
                -- then Lua requires a short run of continued progress.
                if blocked_time <= 0.12 and actual_distance >= 0.35 then
                    self.stuck_escape_clear_timer = f32(
                        self.stuck_escape_clear_timer + step_dt)
                else
                    self.stuck_escape_clear_timer = f32(0.0)
                end

                if self.stuck_escape_clear_timer >= 0.28 then
                    self.stuck_escape_active = false
                    self.stuck_escape_clear_timer = f32(0.0)
                    self.stuck_escape_repath_timer = f32(0.0)
                    self.recovery_timer = f32(0.0)
                    self.recovery_was_active = false
                    if self.fsm:can("to_wander") then
                        -- Discard the target that originally kept the body
                        -- pushing into an icon.
                        transition("wander", frame)
                    end
                elseif self.stuck_escape_repath_timer <= 0.0
                    and clearance > 0.0
                    and length(direction) > 0.001 then
                    -- Refresh the committed route as the body turns around
                    -- nearby icons. A bounded refresh prevents per-frame
                    -- left/right oscillation.
                    self.stuck_escape_direction = f32_vec(direction)
                    self.stuck_escape_distance = f32(clamp(
                        clearance * 0.86,
                        body_length(frame) * 1.35,
                        body_length(frame) * 2.25))
                    self.stuck_escape_repath_timer = f32(0.42)
                end
            end
        end

        -- Short recoveries handle ordinary contacts. If poor real movement
        -- survives for three seconds, promote it to a committed detour that
        -- supersedes the old behavior target until progress is restored.
        if not self.stuck_escape_active
            and not stopped_state
            and blocked_time >= 3.0
            and clearance > 0.0
            and length(direction) > 0.001 then
            local body = body_length(frame)
            local multiplier = config.speed_multiplier or 1.0
            self.stuck_escape_active = true
            self.stuck_escape_direction = f32_vec(direction)
            self.stuck_escape_distance = f32(clamp(
                clearance * 0.86, body * 1.35, body * 2.25))
            self.stuck_escape_speed = f32(
                random("stuck.escape_speed", 188.0, 238.0)
                    * multiplier)
            self.stuck_escape_clear_timer = f32(0.0)
            self.stuck_escape_repath_timer = f32(0.42)
            self.recovery_timer = f32(0.0)
            self.recovery_direction =
                f32_vec(self.stuck_escape_direction)
            return
        end

        if self.stuck_escape_active or self.recovery_timer > 0.0 then
            return
        end

        if clearance > 0.0
            and (blocked_time >= 0.16 or edge_dwell_time >= 0.72)
            and length(direction) > 0.001 then
            self.recovery_direction = f32_vec(direction)
            self.recovery_timer =
                f32(random("solver.recovery_duration", 0.48, 0.72))
        end
    end

    local function recovery_feedback(frame)
        local stopped_state =
            is("lurk")
            or is("groom")
            or is("feeding")
            or is("startled")
        if stopped_state then
            self.recovery_timer = f32(0.0)
            return false, vec()
        end
        if self.stuck_escape_active then
            return true, normalized(self.stuck_escape_direction)
        end
        return self.recovery_timer > 0.0,
            normalized(self.recovery_direction)
    end

    local function avoidance_sensors(frame)
        local sensors = frame.sensors
        local direction = sensors.avoidance_direction
        local urgency = finite_or(sensors.obstacle_urgency, 0.0)
        local moving_urgency =
            finite_or(sensors.moving_obstacle_urgency, 0.0)
        local overlapping = sensors.overlapping == true
        return normalized(direction), urgency, moving_urgency, overlapping
    end

    local function steer(frame)
        local dt = frame.dt
        local body = frame.body
        local position = body_position(frame)
        local body_size = body_length(frame)
        local heading = body.heading or self.initial_heading or 0.0
        local speed = body.speed or 0.0
        local multiplier = config.speed_multiplier or 1.0
        local current_forward = forward(heading)

        -- A mouse threat always takes priority. Otherwise a persistent
        -- collision escape temporarily returns to ordinary roaming so timed
        -- food/corner targets cannot overwrite the detour on every frame.
        if self.stuck_escape_active
            and (is("startled") or is("flee")) then
            self.stuck_escape_active = false
            self.stuck_escape_clear_timer = f32(0.0)
            self.stuck_escape_repath_timer = f32(0.0)
        elseif self.stuck_escape_active
            and not is("wander")
            and self.fsm:can("to_wander") then
            transition("wander", frame)
        end

        if self.stuck_escape_active then
            self.target = f32_vec(add(
                position,
                mul(self.stuck_escape_direction,
                    self.stuck_escape_distance)))
        end
        local direction = normalized(sub(self.target, position))

        local intentionally_still =
            is("pause")
            or is("lurk")
            or is("groom")
            or is("feeding")
            or is("startled")
        local stop_immediately =
            is("lurk")
            or is("groom")
            or is("feeding")
            or is("startled")
        local allow_edge_rest =
            is("seek-corner")
            or is("lurk")
            or is("groom")
        if intentionally_still then
            direction = current_forward
        end

        -- Edge steering is policy. The host still applies the final hard
        -- desktop constraint with the manifest collider.
        local world = world_rect(frame)
        local half_length = body_size * 0.43
        local half_width = body_size * 0.20
        local heading_sin = math.abs(math.sin(heading))
        local heading_cos = math.abs(math.cos(heading))
        local extent_x =
            heading_sin * half_length + heading_cos * half_width
        local extent_y =
            heading_cos * half_length + heading_sin * half_width
        local edge_margin = math.max(72.0, body_size * 0.58)
        local left = position.x - extent_x - world.x
        local right =
            world.x + world.width - position.x - extent_x
        local top = position.y - extent_y - world.y
        local bottom =
            world.y + world.height - position.y - extent_y
        local edge_push = vec()
        if left < edge_margin then
            edge_push.x = edge_push.x + (edge_margin - left) / edge_margin
        end
        if right < edge_margin then
            edge_push.x = edge_push.x - (edge_margin - right) / edge_margin
        end
        if top < edge_margin then
            edge_push.y = edge_push.y + (edge_margin - top) / edge_margin
        end
        if bottom < edge_margin then
            edge_push.y = edge_push.y - (edge_margin - bottom) / edge_margin
        end
        if not allow_edge_rest and length(edge_push) > 0.001 then
            local inward = normalized(edge_push)
            local tangent = vec(-inward.y, inward.x)
            if dot(tangent, current_forward) < 0.0 then
                tangent = mul(tangent, -1.0)
            end
            direction = normalized(add(
                direction,
                add(mul(inward, 2.35), mul(tangent, 0.62))))
        end

        local avoidance, urgency, moving_urgency, overlapping =
            avoidance_sensors(frame)
        urgency = clamp(urgency, 0.0, 1.0)
        moving_urgency = clamp(moving_urgency, 0.0, 1.0)
        if intentionally_still and not overlapping then
            avoidance = vec()
            urgency = 0.0
            moving_urgency = 0.0
        end
        if length(avoidance) > 0.001 then
            direction = normalized(add(
                mul(direction, 1.0 - urgency * 0.58),
                avoidance))
        end

        local recovery_active, recovery_direction =
            recovery_feedback(frame)
        if recovery_active and length(recovery_direction) > 0.001 then
            if self.stuck_escape_active then
                direction = recovery_direction
            else
                direction = normalized(add(
                    mul(direction, 0.16),
                    mul(recovery_direction, 2.85)))
            end
            urgency = math.max(urgency, 0.88)
        end

        if recovery_active and not self.recovery_was_active
            and (is("wander")
                or is("creep")
                or is("pause")) then
            self.desired_speed =
                f32(random("recovery.roaming_speed", 178.0, 248.0)
                    * multiplier)
            self.target = f32_vec(add(
                position,
                mul(recovery_direction,
                    math.max(260.0, body_size * 2.1))))
        end
        self.recovery_was_active = recovery_active

        local desired_heading = heading
        if length(direction) > 0.001 then
            desired_heading = angle_of(direction)
            if is("wander") then
                desired_heading = desired_heading
                    + math.sin(
                        self.behavior_clock * 1.7 + self.steering_phase)
                        * 0.055
                    + math.sin(
                        self.behavior_clock * 4.1 + self.steering_phase)
                        * 0.018
            elseif is("creep")
                or is("seek-food") then
                desired_heading = desired_heading
                    + math.sin(
                        self.behavior_clock * 1.05 + self.steering_phase)
                        * 0.082
                    + math.sin(
                        self.behavior_clock * 2.8 + self.steering_phase)
                        * 0.024
            elseif is("flee") then
                desired_heading = desired_heading
                    + math.sin(
                        self.behavior_clock * 9.0 + self.steering_phase)
                        * 0.075
            end
            desired_heading = wrap_angle(desired_heading)
        end

        local turn_rate
        if is("flee") then
            turn_rate = 8.8
        elseif is("creep")
            or is("seek-corner")
            or is("seek-food") then
            turn_rate = 3.4
        else
            turn_rate = 4.5
        end
        if urgency > 0.0 then
            turn_rate = math.max(
                turn_rate, 5.8 + urgency * 4.8 + moving_urgency * 1.8)
        end
        if recovery_active then
            turn_rate = math.max(turn_rate, 12.5)
        end
        if self.stuck_escape_active then
            turn_rate = math.max(turn_rate, 14.5)
        end

        local desired_speed = self.desired_speed
        if is("wander") then
            local stride_pulse = 0.5 + 0.5 * math.sin(
                self.behavior_clock * 5.2 + self.speed_pulse_phase)
            local pace_drift = 0.5 + 0.5 * math.sin(
                self.behavior_clock * 1.35
                    + self.speed_pulse_phase * 0.61)
            desired_speed = desired_speed
                * (0.72 + stride_pulse * 0.22 + pace_drift * 0.10)
        elseif is("creep")
            or is("seek-corner")
            or is("seek-food") then
            local careful_step = 0.5 + 0.5 * math.sin(
                self.behavior_clock * 3.1 + self.speed_pulse_phase)
            local hesitation = 0.5 + 0.5 * math.sin(
                self.behavior_clock * 0.92
                    + self.speed_pulse_phase * 0.47)
            desired_speed = desired_speed
                * (0.58 + careful_step * 0.26 + hesitation * 0.12)
        elseif is("flee") then
            local pulse = 0.5 + 0.5 * math.sin(
                self.behavior_clock * 10.5 + self.speed_pulse_phase)
            desired_speed = desired_speed * (0.92 + pulse * 0.08)
        end
        if urgency > 0.0 then
            local minimum_escape_speed = multiplier
                * ((moving_urgency > 0.0) and 112.0 or 78.0)
            desired_speed = math.max(
                desired_speed * (1.0 - urgency * 0.18),
                minimum_escape_speed)
        end
        if recovery_active then
            desired_speed =
                math.max(desired_speed, multiplier * 150.0)
        end
        if self.stuck_escape_active then
            desired_speed =
                math.max(desired_speed, self.stuck_escape_speed)
        end

        local acceleration
        if is("flee") then
            acceleration = 1350.0
        elseif is("startled") then
            acceleration = 1550.0
        elseif is("creep") or is("seek-food") then
            acceleration = 520.0
        else
            acceleration = 680.0
        end
        if urgency > 0.0 then
            acceleration = math.max(
                acceleration, 980.0 + moving_urgency * 520.0)
        end
        if self.stuck_escape_active then
            acceleration = math.max(acceleration, 1250.0)
        end

        -- Scuttle uses the speed resulting from this frame's acceleration,
        -- while the gait clock advances last. This ordering is deterministic.
        local predicted_speed = speed + clamp(
            desired_speed - speed, -acceleration * dt, acceleration * dt)
        if stop_immediately then
            predicted_speed = 0.0
        end
        local scuttle = (
            math.sin(self.gait_clock * 2.0) * 0.82
            + math.sin(self.gait_clock * 3.0 + 0.7) * 0.18)
            * math.min(2.8, predicted_speed * 0.0085)
        local cycles_per_second = clamp(
            0.35 + predicted_speed / (body_size * 0.62), 0.35, 5.2)
        self.gait_clock = f32(self.gait_clock
            + dt * cycles_per_second * 2.0 * pi)

        return {
            direction = forward(desired_heading),
            speed = desired_speed,
            turn_rate = turn_rate,
            acceleration = acceleration,
            lateral_speed = scuttle,
            recovery_probe_phase = self.steering_phase * 0.13,
            intentionally_still = intentionally_still,
            stop_immediately = stop_immediately,
            cancel_recovery = stop_immediately,
            allow_edge_rest = allow_edge_rest,
        }
    end

    function self:step(frame)
        if not self.initialized then
            initialize(frame)
        end

        local initial_heading = self.initial_heading
        self.initial_heading = nil
        self.behavior_clock = f32(self.behavior_clock + frame.dt)
        update_host_recovery(frame)
        local consume_bait = update_behavior(frame)
        local motion = steer(frame)
        motion.initial_heading = initial_heading

        return {
            state = self.fsm:current(),
            target = self.target,
            motion = motion,
            events = {
                consume_bait = consume_bait,
            },
        }
    end

    function self:pose(frame)
        local body = frame.body or {}
        local size = body_length(frame)
        local speed = body.speed or 0.0
        local multiplier = config.speed_multiplier or 1.0
        local motion = clamp(
            speed / math.max(1.0, multiplier * 200.0), 0.0, 1.0)

        local probing = 0.42
        if is("pause")
            or is("creep")
            or is("seek-food")
            or is("lurk")
            or is("groom") then
            probing = 1.0
        elseif is("startled") then
            probing = 0.22
        elseif is("flee") then
            probing = 0.08
        elseif is("feeding") then
            probing = 0.75
        end

        local frozen =
            is("lurk")
            or is("groom")
            or is("feeding")
        local tripod = { 0.0, pi, pi, 0.0, 0.0, pi }
        local individual = { 0.13, 1.17, 2.41, 0.73, 1.91, 2.87 }
        local side = { 1.0, -1.0, 1.0, -1.0, 1.0, -1.0 }
        local range_scale = { 1.10, 1.07, 0.82, 0.85, 1.18, 1.14 }
        local names = {
            "left_front_leg",
            "right_front_leg",
            "left_middle_leg",
            "right_middle_leg",
            "left_rear_leg",
            "right_rear_leg",
        }
        local parts = {}
        local stride_range = (2.0 + 18.0 * math.sqrt(motion)) * deg

        for index = 1, 6 do
            local phase = self.gait_clock + tripod[index]
            local individual_motion = math.sin(
                self.gait_clock * 1.83 + individual[index])
            local sweep = math.sin(phase)
                + individual_motion * 0.105
                + math.sin(phase * 2.0 + individual[index]) * 0.055
            local rotation =
                side[index] * sweep * stride_range * range_scale[index]
            local reach = size * (0.0020 + motion * 0.0060)
            local lift =
                math.max(0.0, math.cos(phase)) * size * motion * 0.0035
            local offset = vec(
                side[index] * lift,
                -math.cos(phase) * reach)
            if frozen then
                rotation = 0.0
                offset = vec()
            end
            parts[names[index]] = {
                rotation = rotation,
                joint_offset = offset,
            }
        end

        local antenna_range = (10.0 + probing * 20.0) * deg
        local left_rotation = antenna_range * (
            0.65 * math.sin(self.behavior_clock * 2.55 + 0.21)
            + 0.24 * math.sin(self.behavior_clock * 5.70 + 1.10)
            + 0.11 * math.sin(self.behavior_clock * 11.30))
        local right_rotation = -antenna_range * (
            0.61 * math.sin(self.behavior_clock * 2.13 + 1.37)
            + 0.26 * math.sin(self.behavior_clock * 5.10 + 0.44)
            + 0.13 * math.sin(self.behavior_clock * 10.70 + 2.02))
        local feeler_shift = size * (0.0030 + probing * 0.0040)
        local left_offset = vec(
            -feeler_shift * math.sin(self.behavior_clock * 3.21),
            feeler_shift * math.sin(self.behavior_clock * 4.39 + 0.30))
        local right_offset = vec(
            feeler_shift * math.sin(self.behavior_clock * 2.87 + 1.10),
            feeler_shift * math.sin(self.behavior_clock * 4.07 + 1.90))

        if is("groom") then
            local left_stroke =
                0.5 + 0.5 * math.sin(self.state_clock * 5.4)
            local right_stroke =
                0.5 + 0.5 * math.sin(self.state_clock * 5.4 + pi)
            parts.left_front_leg.rotation =
                (18.0 + left_stroke * 34.0) * deg
            parts.right_front_leg.rotation =
                -(18.0 + right_stroke * 34.0) * deg
            parts.left_front_leg.joint_offset = vec(
                size * left_stroke * 0.018,
                -size * left_stroke * 0.030)
            parts.right_front_leg.joint_offset = vec(
                -size * right_stroke * 0.018,
                -size * right_stroke * 0.030)
            local comb = math.sin(self.state_clock * 5.4)
            left_rotation =
                left_rotation + (8.0 + 7.0 * comb) * deg
            right_rotation =
                right_rotation - (8.0 - 7.0 * comb) * deg
        elseif is("feeding") then
            local nibble =
                0.5 + 0.5 * math.sin(self.state_clock * 8.2)
            parts.left_front_leg.rotation =
                (10.0 + nibble * 12.0) * deg
            parts.right_front_leg.rotation =
                -(10.0 + (1.0 - nibble) * 12.0) * deg
            left_rotation = left_rotation * 0.45
            right_rotation = right_rotation * 0.45
        end

        parts.left_antenna = {
            rotation = left_rotation,
            joint_offset = left_offset,
        }
        parts.right_antenna = {
            rotation = right_rotation,
            joint_offset = right_offset,
        }

        local bob =
            math.sin(self.gait_clock * 2.0) * 0.55 * motion
        local sway = (
            math.sin(self.gait_clock) * 1.15
            + math.sin(self.gait_clock * 2.7) * 0.20) * motion
        local rock = (
            math.sin(self.gait_clock * 2.0) * 1.05
            + math.sin(self.gait_clock * 0.55) * 0.22) * motion * deg

        return {
            body = {
                x = sway,
                y = bob,
                rotation = rock,
            },
            parts = parts,
        }
    end

    return self
end

return {
    api_version = 1,
    new = new_controller,
}
