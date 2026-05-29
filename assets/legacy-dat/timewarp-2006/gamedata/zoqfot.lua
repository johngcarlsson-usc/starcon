-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/zoqfot.jpg"
	DialogSetMusic "gamedata/dialogs/zoqfot.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end