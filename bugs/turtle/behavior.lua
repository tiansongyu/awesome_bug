-- Friendly juvenile tortoise policy and articulated sprite controller.
--
-- Rust owns Windows integration, hard geometry and integration. This module
-- owns every species-specific state, target, speed, recovery policy and pose.

local pi = math.pi
local shell_retract_duration = 0.55
local shell_extend_duration = 0.80

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

local function smoothstep01(value)
    local t = math.max(0.0, math.min(1.0, value))
    return t * t * (3.0 - 2.0 * t)
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
        rest_timer = 0.0,
        interaction_cooldown = 0.0,
        food_retry_timer = 0.0,
        target = vec(),
        pending_direction = vec(),
        feeding_bait_position = vec(),
        steering_phase = 0.0,
        gait_phase = 0.0,
        desired_speed = 0.0,
        initial_heading = nil,
        recovery_timer = 0.0,
        recovery_direction = vec(),
        recovery_was_active = false,
        stuck_escape_active = false,
        stuck_escape_direction = vec(),
        stuck_escape_distance = 0.0,
        stuck_escape_clear_timer = 0.0,
        stuck_escape_repath_timer = 0.0,
    }

    local function random(tag, low, high)
        return host.random(tag, low, high)
    end

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

    local function body_position(frame)
        return position_of(frame.body)
    end

    local function body_length(frame)
        return frame.body.length
    end

    local function feature_enabled(frame, name)
        return frame.features[name] == true
    end

    local function choose_wander_target(frame)
        local world = frame.world
        local body = body_length(frame)
        local margin = math.max(30.0, body * 0.48)
        local margin_x = math.min(margin, world.width * 0.45)
        local margin_y = math.min(margin, world.height * 0.45)
        self.target = f32_vec(vec(
            random(
                "turtle.wander.target.x",
                world.x + margin_x,
                world.x + world.width - margin_x),
            random(
                "turtle.wander.target.y",
                world.y + margin_y,
                world.y + world.height - margin_y)))
    end

    local function choose_corner_target(frame)
        local available = {}
        for index = 1, #frame.corners do
            local corner = frame.corners[index]
            if corner.blocked ~= true then
                available[#available + 1] = corner
            end
        end
        if #available == 0 then
            choose_wander_target(frame)
            return
        end
        local selected = math.floor(random(
            "turtle.corner.choice", 1.0, #available + 0.999))
        selected = math.max(1, math.min(#available, selected))
        self.target = f32_vec(position_of(available[selected]))
    end

    local transition

    local function is(state)
        return self.fsm:is(state)
    end

    local function begin_entry(change)
        self.state_clock = f32(0.0)
        local payload = change.payload or {}
        if type(payload.frame) ~= "table" then
            error("turtle FSM transition requires a frame")
        end
        return payload.frame, payload.direction
    end

    local function enter_wander(_, change)
        local frame = begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        self.state_timer =
            f32(random("turtle.wander.duration", 3.0, 7.0))
        self.desired_speed =
            f32(random("turtle.wander.speed", 38.0, 58.0) * multiplier)
        choose_wander_target(frame)
    end

    local function enter_slow_walk(_, change)
        local frame = begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        self.state_timer =
            f32(random("turtle.slow.duration", 1.8, 4.0))
        self.desired_speed =
            f32(random("turtle.slow.speed", 15.0, 28.0) * multiplier)
        choose_wander_target(frame)
    end

    local function enter_look_around(_, change)
        begin_entry(change)
        self.state_timer =
            f32(random("turtle.look.duration", 1.0, 2.4))
        self.desired_speed = f32(0.0)
    end

    local function enter_pause(_, change)
        begin_entry(change)
        self.state_timer =
            f32(random("turtle.pause.duration", 0.7, 1.8))
        self.desired_speed = f32(0.0)
    end

    local function enter_curious(_, change)
        begin_entry(change)
        self.state_timer =
            f32(random("turtle.curious.duration", 1.0, 2.0))
        self.desired_speed = f32(0.0)
    end

    local function enter_retreat(_, change)
        local frame, direction = begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        local away = normalized(direction or vec())
        if length(away) < 0.001 then
            away = mul(forward((frame.body or {}).heading or 0.0), -1.0)
        end
        self.state_timer =
            f32(random("turtle.retreat.duration", 1.0, 1.8))
        self.desired_speed =
            f32(random("turtle.retreat.speed", 55.0, 78.0) * multiplier)
        self.target = f32_vec(add(
            body_position(frame),
            mul(away, random(
                "turtle.retreat.distance",
                body_length(frame) * 1.1,
                body_length(frame) * 1.8))))
    end

    local function enter_shell_hide(_, change)
        begin_entry(change)
        -- The sampled delay is when the turtle starts emerging. The final
        -- extension then takes shell_extend_duration seconds.
        self.state_timer =
            f32(random("turtle.hide.emerge_delay", 3.0, 10.0)
                + shell_extend_duration)
        self.desired_speed = f32(0.0)
    end

    local function enter_seek_corner(_, change)
        local frame = begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        self.state_timer =
            f32(random("turtle.corner.seek_duration", 12.0, 20.0))
        self.desired_speed =
            f32(random("turtle.corner.speed", 28.0, 43.0) * multiplier)
        choose_corner_target(frame)
    end

    local function enter_corner_rest(_, change)
        begin_entry(change)
        self.state_timer =
            f32(random("turtle.corner.rest_duration", 3.0, 6.0))
        self.desired_speed = f32(0.0)
    end

    local function enter_seek_food(_, change)
        begin_entry(change)
        local multiplier = config.speed_multiplier or 1.0
        self.state_timer =
            f32(random("turtle.food.seek_duration", 16.0, 25.0))
        self.desired_speed =
            f32(random("turtle.food.seek_speed", 24.0, 39.0) * multiplier)
    end

    local function enter_feeding(_, change)
        begin_entry(change)
        self.state_timer =
            f32(random("turtle.food.feed_duration", 2.8, 4.5))
        self.desired_speed = f32(0.0)
    end

    local function create_fsm(frame)
        local definition = {
            states = {
                ["wander"] = { enter = enter_wander },
                ["slow-walk"] = { enter = enter_slow_walk },
                ["look-around"] = { enter = enter_look_around },
                ["pause"] = { enter = enter_pause },
                ["curious"] = { enter = enter_curious },
                ["retreat"] = { enter = enter_retreat },
                ["shell-hide"] = { enter = enter_shell_hide },
                ["seek-corner"] = { enter = enter_seek_corner },
                ["corner-rest"] = { enter = enter_corner_rest },
                ["seek-food"] = { enter = enter_seek_food },
                ["feeding"] = { enter = enter_feeding },
            },
            events = {
                to_wander = { from = "*", to = "wander" },
                ["to_slow-walk"] = { from = "*", to = "slow-walk" },
                ["to_look-around"] = { from = "*", to = "look-around" },
                to_pause = { from = "*", to = "pause" },
                to_curious = { from = "*", to = "curious" },
                to_retreat = { from = "*", to = "retreat" },
                ["to_shell-hide"] = { from = "*", to = "shell-hide" },
                ["to_seek-corner"] = { from = "*", to = "seek-corner" },
                ["to_corner-rest"] = { from = "*", to = "corner-rest" },
                ["to_seek-food"] = { from = "*", to = "seek-food" },
                to_feeding = { from = "*", to = "feeding" },
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
        self.initial_heading =
            f32(random("turtle.init.heading", -pi, pi))
        self.behavior_clock =
            f32(random("turtle.init.behavior_clock", 0.0, 20.0))
        self.gait_clock =
            f32(random("turtle.init.gait_clock", 0.0, 2.0 * pi))
        self.gait_phase =
            f32(random("turtle.init.gait_phase", -pi, pi))
        self.steering_phase =
            f32(random("turtle.init.steering_phase", -pi, pi))
        self.rest_timer =
            f32(random("turtle.init.rest_timer", 28.0, 52.0))
        self.fsm = create_fsm(frame)
        self.initialized = true
    end

    local function choose_roaming_behavior(frame)
        local choice = random("turtle.roaming.choice", 0.0, 1.0)
        if choice < 0.12 then
            transition("pause", frame)
        elseif choice < 0.25 then
            transition("look-around", frame)
        elseif choice < 0.43 then
            transition("slow-walk", frame)
        else
            transition("wander", frame)
        end
    end

    local function update_recovery(frame)
        local feedback = frame.feedback or {}
        local dt = math.max(0.0, finite_or(frame.dt, 0.0))
        local blocked_time = finite_or(feedback.blocked_time, 0.0)
        local edge_time = finite_or(feedback.edge_dwell_time, 0.0)
        local clearance =
            finite_or(feedback.recovery_clearance, 0.0)
        local direction =
            normalized(feedback.recovery_direction or vec())
        local actual_distance =
            length(feedback.actual_displacement or vec())

        self.recovery_timer =
            f32(math.max(0.0, self.recovery_timer - dt))
        self.stuck_escape_repath_timer = f32(math.max(
            0.0, self.stuck_escape_repath_timer - dt))

        local intentionally_resting =
            is("pause")
            or is("look-around")
            or is("curious")
            or is("shell-hide")
            or is("corner-rest")
            or is("feeding")

        if self.stuck_escape_active then
            if intentionally_resting then
                self.stuck_escape_active = false
                self.stuck_escape_clear_timer = f32(0.0)
            else
                if blocked_time <= 0.12 and actual_distance >= 0.22 then
                    self.stuck_escape_clear_timer =
                        f32(self.stuck_escape_clear_timer + dt)
                else
                    self.stuck_escape_clear_timer = f32(0.0)
                end

                if self.stuck_escape_clear_timer >= 0.30 then
                    self.stuck_escape_active = false
                    self.stuck_escape_clear_timer = f32(0.0)
                    self.recovery_timer = f32(0.0)
                    self.recovery_was_active = false
                    transition("wander", frame)
                elseif self.stuck_escape_repath_timer <= 0.0
                    and clearance > 0.0
                    and length(direction) > 0.001 then
                    self.stuck_escape_direction = f32_vec(direction)
                    self.stuck_escape_distance = f32(clamp(
                        clearance * 0.82,
                        body_length(frame) * 1.15,
                        body_length(frame) * 2.0))
                    self.stuck_escape_repath_timer = f32(0.55)
                end
            end
        end

        if not self.stuck_escape_active
            and not intentionally_resting
            and blocked_time >= 3.0
            and clearance > 0.0
            and length(direction) > 0.001 then
            self.stuck_escape_active = true
            self.stuck_escape_direction = f32_vec(direction)
            self.stuck_escape_distance = f32(clamp(
                clearance * 0.82,
                body_length(frame) * 1.15,
                body_length(frame) * 2.0))
            self.stuck_escape_clear_timer = f32(0.0)
            self.stuck_escape_repath_timer = f32(0.55)
            self.recovery_timer = f32(0.0)
            transition("wander", frame)
            return
        end

        if self.stuck_escape_active or self.recovery_timer > 0.0 then
            return
        end
        if clearance > 0.0
            and (blocked_time >= 0.18 or edge_time >= 0.82)
            and length(direction) > 0.001 then
            self.recovery_direction = f32_vec(direction)
            self.recovery_timer =
                f32(random("turtle.recovery.duration", 0.75, 1.15))
        end
    end

    local function cancel_escape()
        self.stuck_escape_active = false
        self.stuck_escape_clear_timer = f32(0.0)
        self.stuck_escape_repath_timer = f32(0.0)
        self.recovery_timer = f32(0.0)
        self.recovery_was_active = false
    end

    local function update_behavior(frame)
        local dt = math.max(0.0, frame.dt)
        local position = body_position(frame)
        local body = body_length(frame)
        local extended =
            feature_enabled(frame, "extended_behaviors")
        local cursor = frame.cursor or {}
        local cursor_position = position_of(cursor)
        local cursor_velocity = velocity_of(cursor)
        local cursor_delta = sub(position, cursor_position)
        local cursor_distance = length(cursor_delta)
        local cursor_speed = length(cursor_velocity)
        local cursor_away = normalized(cursor_delta)
        local approach_speed = dot(cursor_velocity, cursor_away)

        self.state_timer = f32(self.state_timer - dt)
        self.state_clock = f32(self.state_clock + dt)
        self.interaction_cooldown = f32(math.max(
            0.0, self.interaction_cooldown - dt))
        self.food_retry_timer = f32(math.max(
            0.0, self.food_retry_timer - dt))
        if not is("seek-corner")
            and not is("corner-rest")
            and not is("seek-food")
            and not is("feeding") then
            self.rest_timer = f32(self.rest_timer - dt)
        end

        local cursor_valid = cursor.valid == true
        local clicked_near =
            cursor_valid
            and cursor.left_button_pressed == true
            and cursor_distance < body * 0.68
        local rapid_approach =
            cursor_valid
            and cursor_speed >= 420.0
            and approach_speed >= 220.0
            and cursor_distance < body * 2.4
        local uncomfortably_close =
            cursor_valid and cursor_distance < body * 0.68
        local can_interact =
            self.interaction_cooldown <= 0.0
            and not is("shell-hide")
            and not is("retreat")
            and not is("feeding")

        if extended and clicked_near and can_interact then
            cancel_escape()
            self.interaction_cooldown = f32(2.2)
            transition("shell-hide", frame)
        elseif extended
            and (rapid_approach or uncomfortably_close)
            and can_interact then
            cancel_escape()
            self.interaction_cooldown = f32(1.5)
            transition("retreat", frame, cursor_delta)
        elseif extended
            and cursor_valid
            and cursor_speed < 220.0
            and cursor_distance >= body * 0.72
            and cursor_distance < body * 1.9
            and can_interact
            and (is("wander")
                or is("slow-walk")
                or is("look-around")
                or is("pause")) then
            self.interaction_cooldown = f32(2.0)
            transition("curious", frame)
        end

        if self.stuck_escape_active then
            return false
        end

        local bait = frame.bait or {}
        local bait_position = position_of(bait)
        local bait_active = bait.active == true
        local can_seek_food =
            not is("retreat")
            and not is("shell-hide")
            and not is("feeding")
            and not is("seek-food")
        if extended
            and bait_active
            and can_seek_food
            and self.food_retry_timer <= 0.0 then
            self.target = f32_vec(bait_position)
            transition("seek-food", frame)
        end

        if is("seek-food") then
            if not bait_active or (frame.sensors or {}).bait_blocked == true then
                self.food_retry_timer = f32(2.0)
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
            elseif length(sub(
                bait_position, self.feeding_bait_position)) > 2.0 then
                self.target = f32_vec(bait_position)
                transition("seek-food", frame)
            end
        end

        if extended
            and self.rest_timer <= 0.0
            and not bait_active
            and (is("wander")
                or is("slow-walk")
                or is("pause")
                or is("look-around")) then
            transition("seek-corner", frame)
        end
        if is("seek-corner")
            and length(sub(self.target, position)) < body * 0.36 then
            transition("corner-rest", frame)
        end

        local consume_bait = false
        if self.state_timer <= 0.0 then
            if is("shell-hide")
                or is("retreat")
                or is("curious") then
                transition("wander", frame)
            elseif is("pause")
                or is("look-around")
                or is("slow-walk")
                or is("wander") then
                choose_roaming_behavior(frame)
            elseif is("seek-corner") then
                choose_corner_target(frame)
                self.state_timer =
                    f32(random(
                        "turtle.corner.retry_duration", 8.0, 14.0))
            elseif is("corner-rest") then
                self.rest_timer =
                    f32(random("turtle.corner.next_rest", 32.0, 62.0))
                transition("wander", frame)
            elseif is("seek-food") then
                self.food_retry_timer =
                    f32(random("turtle.food.retry", 3.0, 6.0))
                transition("wander", frame)
            elseif is("feeding") then
                consume_bait = true
                self.rest_timer =
                    f32(random("turtle.food.next_rest", 18.0, 36.0))
                transition("look-around", frame)
            end
        end

        if (is("wander") or is("slow-walk"))
            and length(sub(self.target, position)) < body * 0.42 then
            choose_roaming_behavior(frame)
        end
        return consume_bait
    end

    local function recovery_feedback()
        local stopped =
            is("pause")
            or is("look-around")
            or is("curious")
            or is("shell-hide")
            or is("corner-rest")
            or is("feeding")
        if stopped then
            self.recovery_timer = f32(0.0)
            return false, vec()
        end
        if self.stuck_escape_active then
            return true, normalized(self.stuck_escape_direction)
        end
        return self.recovery_timer > 0.0,
            normalized(self.recovery_direction)
    end

    local function steer(frame)
        local dt = math.max(0.0, frame.dt)
        local body = frame.body
        local position = body_position(frame)
        local size = body_length(frame)
        local heading = body.heading or self.initial_heading or 0.0
        local speed = body.speed or 0.0
        local multiplier = config.speed_multiplier or 1.0
        local current_forward = forward(heading)

        if self.stuck_escape_active then
            self.target = f32_vec(add(
                position,
                mul(self.stuck_escape_direction,
                    self.stuck_escape_distance)))
        end
        local direction = normalized(sub(self.target, position))

        local stopped =
            is("pause")
            or is("look-around")
            or is("curious")
            or is("shell-hide")
            or is("corner-rest")
            or is("feeding")
        local allow_edge_rest =
            is("seek-corner") or is("corner-rest")
        if stopped then
            direction = current_forward
        end

        local world = frame.world
        local extent_x = size * 0.31
        local extent_y = size * 0.37
        local margin = math.max(48.0, size * 0.52)
        local edge_push = vec()
        local left = position.x - extent_x - world.x
        local right =
            world.x + world.width - position.x - extent_x
        local top = position.y - extent_y - world.y
        local bottom =
            world.y + world.height - position.y - extent_y
        if left < margin then
            edge_push.x = edge_push.x + (margin - left) / margin
        end
        if right < margin then
            edge_push.x = edge_push.x - (margin - right) / margin
        end
        if top < margin then
            edge_push.y = edge_push.y + (margin - top) / margin
        end
        if bottom < margin then
            edge_push.y = edge_push.y - (margin - bottom) / margin
        end
        if not allow_edge_rest and length(edge_push) > 0.001 then
            local inward = normalized(edge_push)
            local tangent = vec(-inward.y, inward.x)
            if dot(tangent, current_forward) < 0.0 then
                tangent = mul(tangent, -1.0)
            end
            direction = normalized(add(
                direction,
                add(mul(inward, 2.1), mul(tangent, 0.55))))
        end

        local sensors = frame.sensors or {}
        local avoidance =
            normalized(sensors.avoidance_direction or vec())
        local urgency = clamp(
            finite_or(sensors.obstacle_urgency, 0.0), 0.0, 1.0)
        local moving_urgency = clamp(
            finite_or(sensors.moving_obstacle_urgency, 0.0),
            0.0,
            1.0)
        local overlapping = sensors.overlapping == true
        if stopped and not overlapping then
            avoidance = vec()
            urgency = 0.0
            moving_urgency = 0.0
        end
        if length(avoidance) > 0.001 then
            direction = normalized(add(
                mul(direction, 1.0 - urgency * 0.68),
                mul(avoidance, 0.85 + urgency * 0.55)))
        end

        local recovery_active, recovery_direction =
            recovery_feedback()
        if recovery_active and length(recovery_direction) > 0.001 then
            if self.stuck_escape_active then
                direction = recovery_direction
            else
                direction = normalized(add(
                    mul(direction, 0.22),
                    mul(recovery_direction, 2.2)))
            end
            urgency = math.max(urgency, 0.78)
        end

        if recovery_active and not self.recovery_was_active then
            self.target = f32_vec(add(
                position,
                mul(recovery_direction,
                    math.max(size * 1.35, 150.0))))
        end
        self.recovery_was_active = recovery_active

        local desired_heading = heading
        if length(direction) > 0.001 then
            desired_heading = angle_of(direction)
            if is("wander") or is("slow-walk") then
                desired_heading = desired_heading
                    + math.sin(
                        self.behavior_clock * 0.85
                            + self.steering_phase)
                        * 0.028
            end
            desired_heading = wrap_angle(desired_heading)
        end

        local turn_rate
        if is("retreat") then
            turn_rate = 3.2
        elseif is("slow-walk")
            or is("seek-food")
            or is("seek-corner") then
            turn_rate = 1.15
        else
            turn_rate = 1.55
        end
        if urgency > 0.0 then
            turn_rate = math.max(
                turn_rate, 2.2 + urgency * 2.2
                    + moving_urgency * 0.8)
        end
        if recovery_active then
            turn_rate = math.max(turn_rate, 4.8)
        end
        if self.stuck_escape_active then
            turn_rate = math.max(turn_rate, 7.0)
        end

        local desired_speed = self.desired_speed
        if is("wander") then
            local pace = 0.5 + 0.5 * math.sin(
                self.behavior_clock * 1.8 + self.gait_phase)
            desired_speed = desired_speed * (0.90 + pace * 0.10)
        elseif is("slow-walk")
            or is("seek-food")
            or is("seek-corner") then
            local step = 0.5 + 0.5 * math.sin(
                self.behavior_clock * 1.35 + self.gait_phase)
            desired_speed = desired_speed * (0.84 + step * 0.12)
        elseif is("retreat") then
            local hurry = 0.5 + 0.5 * math.sin(
                self.behavior_clock * 3.2 + self.gait_phase)
            desired_speed = desired_speed * (0.92 + hurry * 0.08)
        end
        if urgency > 0.0 then
            local minimum = multiplier
                * ((moving_urgency > 0.0) and 44.0 or 28.0)
            desired_speed =
                math.max(desired_speed * (1.0 - urgency * 0.28), minimum)
        end
        if recovery_active then
            desired_speed =
                math.max(desired_speed, multiplier * 42.0)
        end
        if self.stuck_escape_active then
            desired_speed =
                math.max(desired_speed, multiplier * 58.0)
        end

        local acceleration
        if is("retreat") then
            acceleration = 180.0
        elseif is("slow-walk") or is("seek-food") then
            acceleration = 70.0
        else
            acceleration = 105.0
        end
        if urgency > 0.0 then
            acceleration = math.max(
                acceleration, 145.0 + moving_urgency * 70.0)
        end
        if self.stuck_escape_active then
            acceleration = math.max(acceleration, 240.0)
        end

        local predicted_speed = speed + clamp(
            desired_speed - speed,
            -acceleration * dt,
            acceleration * dt)
        if stopped then
            predicted_speed = 0.0
        end
        local cycles_per_second = clamp(
            0.18 + predicted_speed / math.max(1.0, size * 0.58),
            0.18,
            1.7)
        self.gait_clock = f32(
            self.gait_clock + dt * cycles_per_second * 2.0 * pi)
        local lateral = math.sin(self.gait_clock * 2.0)
            * math.min(0.65, predicted_speed * 0.009)

        return {
            direction = forward(desired_heading),
            speed = desired_speed,
            turn_rate = turn_rate,
            acceleration = acceleration,
            lateral_speed = lateral,
            recovery_probe_phase = self.steering_phase * 0.11,
            intentionally_still = stopped,
            stop_immediately = stopped,
            cancel_recovery = stopped,
            allow_edge_rest = allow_edge_rest,
        }
    end

    function self:step(frame)
        if not self.initialized then
            initialize(frame)
        end
        local initial_heading = self.initial_heading
        self.initial_heading = nil
        self.behavior_clock =
            f32(self.behavior_clock + math.max(0.0, frame.dt))
        update_recovery(frame)
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
        local speed = finite_or(body.speed, 0.0)
        local multiplier = config.speed_multiplier or 1.0
        local motion =
            clamp(speed / math.max(1.0, multiplier * 58.0), 0.0, 1.0)
        local moving =
            is("wander")
            or is("slow-walk")
            or is("retreat")
            or is("seek-corner")
            or is("seek-food")
        local pose_motion = moving and motion or 0.0
        local stride = moving
            and math.sin(self.gait_clock + self.gait_phase) * motion
            or 0.0
        local secondary = moving
            and math.sin(
                self.gait_clock * 2.0 + self.gait_phase * 0.7)
                * motion
            or 0.0

        local shell_sway =
            math.sin(self.gait_clock * 2.0) * 0.012 * pose_motion
        local body_offset = vec(
            math.sin(self.gait_clock * 2.0) * 0.22 * pose_motion,
            math.cos(self.gait_clock) * 0.14 * pose_motion)
        local shell_tuck = 0.0
        if is("shell-hide") then
            local retract =
                smoothstep01(self.state_clock / shell_retract_duration)
            local extend =
                smoothstep01(self.state_timer / shell_extend_duration)
            shell_tuck = retract * extend
        end

        local head_rotation =
            math.sin(self.behavior_clock * 0.75 + self.steering_phase)
                * 0.035
        local head_offset = vec()
        if is("look-around") or is("corner-rest") then
            head_rotation =
                math.sin(self.state_clock * 1.7 + self.steering_phase)
                    * 0.17
        elseif is("curious") then
            local cursor = frame.cursor or {}
            local look = normalized(sub(
                position_of(cursor), body_position(frame)))
            if length(look) > 0.001 then
                head_rotation = clamp(
                    wrap_angle(angle_of(look) - (body.heading or 0.0)),
                    -0.20,
                    0.20)
            end
            head_offset.y = -size * 0.025
        elseif is("retreat") then
            head_offset.y = size * 0.085
            head_rotation =
                math.sin(self.state_clock * 4.0) * 0.035
        elseif is("shell-hide") then
            head_offset.y = size * 0.225 * shell_tuck
            head_rotation = head_rotation * (1.0 - shell_tuck)
        elseif is("feeding") then
            head_offset.y =
                -size * (0.035 + 0.018
                    * math.sin(self.state_clock * 4.2))
            head_rotation =
                math.sin(self.state_clock * 2.1) * 0.025
        end

        local left_front_offset = vec()
        local right_front_offset = vec()
        local left_rear_offset = vec()
        local right_rear_offset = vec()
        local tail_offset = vec()
        if is("shell-hide") then
            left_front_offset =
                vec(size * 0.16 * shell_tuck,
                    size * 0.07 * shell_tuck)
            right_front_offset =
                vec(-size * 0.16 * shell_tuck,
                    size * 0.07 * shell_tuck)
            left_rear_offset =
                vec(size * 0.15 * shell_tuck,
                    -size * 0.06 * shell_tuck)
            right_rear_offset =
                vec(-size * 0.15 * shell_tuck,
                    -size * 0.06 * shell_tuck)
            tail_offset.y = -size * 0.10 * shell_tuck
        elseif is("pause") or is("corner-rest") then
            local settle =
                math.sin(self.behavior_clock * 0.9) * size * 0.003
            left_front_offset.y = settle
            right_front_offset.y = -settle
        end

        local front_range = 0.15
        local rear_range = 0.11
        local left_front_rotation =
            stride * front_range + secondary * 0.025
        local right_front_rotation =
            -stride * front_range - secondary * 0.025
        local left_rear_rotation =
            -stride * rear_range + secondary * 0.018
        local right_rear_rotation =
            stride * rear_range - secondary * 0.018
        if is("shell-hide") then
            left_front_rotation = 0.10 * shell_tuck
            right_front_rotation = -0.10 * shell_tuck
            left_rear_rotation = -0.08 * shell_tuck
            right_rear_rotation = 0.08 * shell_tuck
        end

        local tail_rotation =
            math.sin(self.behavior_clock * 0.8 + self.gait_phase)
                * (0.025 + motion * 0.025)
        if is("shell-hide") then
            tail_rotation = tail_rotation * (1.0 - shell_tuck)
        end
        return {
            body = {
                x = body_offset.x,
                y = body_offset.y,
                rotation = shell_sway,
            },
            parts = {
                body = {
                    rotation = 0.0,
                    joint_offset = vec(),
                },
                head = {
                    rotation = head_rotation,
                    joint_offset = head_offset,
                },
                left_front_leg = {
                    rotation = left_front_rotation,
                    joint_offset = left_front_offset,
                },
                right_front_leg = {
                    rotation = right_front_rotation,
                    joint_offset = right_front_offset,
                },
                left_rear_leg = {
                    rotation = left_rear_rotation,
                    joint_offset = left_rear_offset,
                },
                right_rear_leg = {
                    rotation = right_rear_rotation,
                    joint_offset = right_rear_offset,
                },
                tail = {
                    rotation = tail_rotation,
                    joint_offset = tail_offset,
                },
            },
        }
    end

    return self
end

return {
    api_version = 1,
    new = new_controller,
}
