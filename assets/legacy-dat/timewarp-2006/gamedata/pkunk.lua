-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/pkunk.jpg"
	DialogSetMusic "gamedata/dialogs/pkunk.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end