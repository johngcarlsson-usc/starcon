-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/korah.lua"
	DialogSetMusic "gamedata/dialogs/korah.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end