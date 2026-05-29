-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/druuge.jpg"
	DialogSetMusic "gamedata/dialogs/druuge.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end