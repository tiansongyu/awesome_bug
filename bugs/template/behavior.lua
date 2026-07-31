-- Minimal behavior ABI v1 implementation. Persistent per-instance state lives
-- in the controller returned by `new`; module-level mutable state is forbidden.
return {
    api_version = 1,

    new = function(config, host)
        if type(host.fsm) ~= "table"
            or type(host.fsm.create) ~= "function" then
            error("host.fsm.create is required by behavior ABI v1")
        end

        local self = {
            heading = host.random("template.heading", -math.pi, math.pi),
            first_step = true,
            timer = 0.0,
        }
        self.fsm = host.fsm.create({
            states = {
                moving = {
                    enter = function(context)
                        context.timer = 3.0
                    end,
                },
                resting = {
                    enter = function(context)
                        context.timer = 1.0
                    end,
                },
            },
            events = {
                rest = { from = { "moving" }, to = "resting" },
                move = { from = { "resting" }, to = "moving" },
            },
        }, "moving", self)

        function self:step(frame)
            self.timer = self.timer - frame.dt
            if self.timer <= 0.0 then
                if self.fsm:is("moving") then
                    self.fsm:send("rest")
                else
                    self.fsm:send("move")
                end
            end

            local initial_heading
            if self.first_step then
                initial_heading = self.heading
                self.first_step = false
            end
            local moving = self.fsm:is("moving")
            return {
                state = self.fsm:current(),
                target = {
                    x = frame.body.x,
                    y = frame.body.y,
                },
                motion = {
                    direction = {
                        x = math.sin(self.heading),
                        y = -math.cos(self.heading),
                    },
                    speed = moving and 80.0 or 0.0,
                    turn_rate = 2.0,
                    acceleration = 240.0,
                    lateral_speed = 0.0,
                    intentionally_still = not moving,
                    stop_immediately = not moving,
                    cancel_recovery = not moving,
                    allow_edge_rest = false,
                    initial_heading = initial_heading,
                },
                events = {
                    consume_bait = false,
                },
            }
        end

        function self:pose(frame)
            return {
                body = { x = 0.0, y = 0.0, rotation = 0.0 },
                parts = {},
            }
        end

        return self
    end,
}
