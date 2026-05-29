-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/mycon.jpg"
	DialogSetMusic "gamedata/dialogs/mycon.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end