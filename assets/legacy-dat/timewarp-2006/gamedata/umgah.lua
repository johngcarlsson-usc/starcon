-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/umgah.jpg"
	DialogSetMusic "gamedata/dialogs/umgah.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end