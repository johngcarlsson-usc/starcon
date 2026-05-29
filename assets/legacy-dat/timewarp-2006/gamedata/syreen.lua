-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/syreen.jpg"
	DialogSetMusic "gamedata/dialogs/syreen.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end