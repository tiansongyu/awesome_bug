-- Small event-driven finite-state machine used by species controllers.
--
-- A definition has `states` and `events` maps:
--
--   local machine = fsm.create({
--       states = {
--           idle = {
--               enter = function(context, transition) end,
--               leave = function(context, transition) end,
--           },
--           moving = {},
--       },
--       events = {
--           move = { from = { "idle" }, to = "moving" },
--           stop = { from = { "moving" }, to = "idle" },
--       },
--   }, "idle", context, initial_payload)
--
-- `send` either completes one transition or raises an explicit error. State is
-- changed between leave and enter callbacks, so each callback observes the
-- phase implied by its name. A failed leave callback keeps the old state. A
-- failed enter callback is not rolled back: callers must treat callback errors
-- as controller failures, which is exactly how the Lua host quarantines bugs.

local function expect_table(value, name)
    if type(value) ~= "table" then
        error(name .. " must be a table", 3)
    end
end

local function valid_source(rule, current)
    local from = rule.from
    if from == "*" then
        return true
    end
    if type(from) == "string" then
        return from == current
    end
    if type(from) == "table" then
        for index = 1, #from do
            if from[index] == current then
                return true
            end
        end
    end
    return false
end

local function validate_definition(definition)
    for name, state_definition in pairs(definition.states) do
        if type(name) ~= "string" or name == "" then
            error("FSM state names must be non-empty strings", 3)
        end
        if type(state_definition) ~= "table" then
            error("FSM state " .. name .. " must be a table", 3)
        end
        if state_definition.enter ~= nil
            and type(state_definition.enter) ~= "function" then
            error("FSM enter callback for " .. name
                .. " must be a function", 3)
        end
        if state_definition.leave ~= nil
            and type(state_definition.leave) ~= "function" then
            error("FSM leave callback for " .. name
                .. " must be a function", 3)
        end
    end

    for event, rule in pairs(definition.events) do
        if type(event) ~= "string" or event == "" then
            error("FSM event names must be non-empty strings", 3)
        end
        if type(rule) ~= "table" then
            error("FSM event " .. event .. " must be a table", 3)
        end
        if type(rule.to) ~= "string"
            or definition.states[rule.to] == nil then
            error("FSM event " .. event
                .. " has an undefined destination", 3)
        end
        local from = rule.from
        if from ~= "*" and type(from) ~= "string"
            and type(from) ~= "table" then
            error("FSM event " .. event
                .. " has an invalid source set", 3)
        end
        if type(from) == "string" and from ~= "*"
            and definition.states[from] == nil then
            error("FSM event " .. event
                .. " has an undefined source " .. from, 3)
        elseif type(from) == "table" then
            if #from == 0 then
                error("FSM event " .. event
                    .. " has an empty source set", 3)
            end
            for index = 1, #from do
                local source = from[index]
                if type(source) ~= "string"
                    or definition.states[source] == nil then
                    error("FSM event " .. event
                        .. " has an undefined source "
                        .. tostring(source), 3)
                end
            end
        end
    end
end

local function create(definition, initial, context, initial_payload)
    expect_table(definition, "FSM definition")
    expect_table(definition.states, "FSM states")
    expect_table(definition.events, "FSM events")
    if type(initial) ~= "string" or definition.states[initial] == nil then
        error("FSM initial state is not defined: " .. tostring(initial), 2)
    end
    validate_definition(definition)

    local state = initial
    local transitioning = false
    local machine = {}

    function machine:current()
        return state
    end

    function machine:is(candidate)
        return state == candidate
    end

    function machine:can(event)
        local rule = definition.events[event]
        return type(rule) == "table"
            and definition.states[rule.to] ~= nil
            and valid_source(rule, state)
    end

    function machine:send(event, payload)
        if transitioning then
            error("FSM transitions cannot be re-entered", 2)
        end
        local rule = definition.events[event]
        if type(rule) ~= "table" then
            error("FSM event is not defined: " .. tostring(event), 2)
        end
        if type(rule.to) ~= "string"
            or definition.states[rule.to] == nil then
            error(
                "FSM event " .. tostring(event)
                    .. " has an undefined destination",
                2)
        end
        if not valid_source(rule, state) then
            error(
                "FSM event " .. tostring(event)
                    .. " is illegal from state " .. state,
                2)
        end

        local previous = state
        local transition = {
            event = event,
            from = previous,
            to = rule.to,
            payload = payload,
        }
        transitioning = true
        local leave = definition.states[previous].leave
        if leave ~= nil then
            if type(leave) ~= "function" then
                error("FSM leave callback must be a function", 2)
            end
            leave(context, transition)
        end

        state = rule.to
        local enter = definition.states[state].enter
        if enter ~= nil then
            if type(enter) ~= "function" then
                error("FSM enter callback must be a function", 2)
            end
            enter(context, transition)
        end
        transitioning = false
        return state
    end

    local initial_enter = definition.states[state].enter
    if initial_enter ~= nil then
        if type(initial_enter) ~= "function" then
            error("FSM enter callback must be a function", 2)
        end
        initial_enter(context, {
            event = "__init",
            from = nil,
            to = state,
            payload = initial_payload,
        })
    end

    return machine
end

return {
    api_version = 1,
    create = create,
}
