sciink — fast Inkscape extensions for scientific figures
https://github.com/Mr-Milk/sciink

INSTALL
  Unzip so that this folder (sciink/) sits directly inside Inkscape's user
  extensions directory (Edit > Preferences > System > User extensions):
    macOS    ~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink
    Linux    ~/.config/inkscape/extensions/sciink
    Windows  %APPDATA%\inkscape\extensions\sciink
  Restart Inkscape. The tools appear under Extensions > Scientific.

  Or use the one-line installers from the project README:
    macOS/Linux:  curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh
    Windows:      irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1 | iex

macOS: if the menu entries do nothing after a browser download, clear the
quarantine flag once:
  xattr -dr com.apple.quarantine "$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink"
(The curl installer never sets that flag.)

VERSION  @VERSION@
