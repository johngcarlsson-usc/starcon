-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/sylandro.jpg"
	DialogSetMusic "gamedata/dialogs/sylandro.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end