-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/dynarry.jpg"
	DialogSetMusic "gamedata/dialogs/dynarry.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end