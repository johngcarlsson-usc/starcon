-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/orz.jpg"
	DialogSetMusic "gamedata/dialogs/orz.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end