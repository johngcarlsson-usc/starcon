-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/thraddash.jpg"
	DialogSetMusic "gamedata/dialogs/thraddash.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end