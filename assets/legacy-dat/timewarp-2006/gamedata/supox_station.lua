-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/supox.jpg"
	DialogSetMusic "gamedata/dialogs/supox.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end