-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/shofixty.jpg"
	DialogSetMusic "gamedata/dialogs/shofixty.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end