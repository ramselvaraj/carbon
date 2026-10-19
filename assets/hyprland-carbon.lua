-- Carbon draws its own translucent backgrounds, so keep Hyprland's window-wide opacity off:
-- it would dim the text too. Blur still shows through the see-through pixels.
-- (Add to ~/.config/hypr/looknfeel.lua, where `o` is Omarchy's rule helper.)
o.window({ class = "^carbon$" }, { tag = "-default-opacity", opacity = "1.0 1.0" })
