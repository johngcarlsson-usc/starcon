-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/yehat.jpg"
	DialogSetMusic "gamedata/dialogs/yehat.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end