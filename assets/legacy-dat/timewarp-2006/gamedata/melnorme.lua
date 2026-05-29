-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/melnorme.jpg"
	DialogSetMusic "gamedata/dialogs/melnorme.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end