-- Tabify: re-indents the open file with tabs instead of spaces.

-- How many spaces make one level: the smallest indent of two or more spaces among the lines
-- indented with spaces, 4 when there are none.
local function indent_width(text)
	local width = nil
	for line in string.gmatch(text, "[^\n]+") do
		local spaces = string.match(line, "^( +)%S")
		if spaces and #spaces > 1 and (width == nil or #spaces < width) then
			width = #spaces
		end
	end
	return width or 4
end

-- The line with its leading spaces and tabs turned into tabs (and the spaces left over, when
-- the indent isn't a whole number of levels). Lines of only whitespace are left alone.
local function retab(line, width)
	local lead, rest = string.match(line, "^([ \t]*)(.*)$")
	if lead == "" or rest == "" or string.match(rest, "^%s*$") then
		return line, false
	end
	local columns = 0
	for c in string.gmatch(lead, ".") do
		if c == "\t" then
			columns = columns + width - columns % width
		else
			columns = columns + 1
		end
	end
	local new = string.rep("\t", math.floor(columns / width)) .. string.rep(" ", columns % width) .. rest
	return new, new ~= line
end

codecraft.command("format-with-tabs", "Format Document with Tabs", function(doc)
	local width = indent_width(doc.text)
	local changed = 0
	local text = string.gsub(doc.text, "[^\n]+", function(line)
		local new, did = retab(line, width)
		if did then
			changed = changed + 1
		end
		return new
	end)
	if changed == 0 then
		codecraft.notify(doc.name .. " is already indented with tabs")
		return nil
	end
	codecraft.notify(string.format("%d lines re-indented with tabs (%d spaces each)", changed, width))
	return text
end)
