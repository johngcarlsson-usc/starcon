-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/ilwrath.jpg"
	DialogSetMusic "gamedata/dialogs/ilwrath.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end