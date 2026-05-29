-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/human.jpg"
	DialogSetMusic "gamedata/dialogs/human.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end