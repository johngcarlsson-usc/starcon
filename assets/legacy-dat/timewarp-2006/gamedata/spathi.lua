-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/spathi.jpg"
	DialogSetMusic "gamedata/dialogs/spathi.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end