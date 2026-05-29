-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/chmmr.jpg"
	DialogSetMusic "gamedata/dialogs/chmmr.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end