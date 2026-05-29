-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/urquan.jpg"
	DialogSetMusic "gamedata/dialogs/urquan.mod"
	DialogWrite "Hi!"
	answer = DialogAnswer ( "Bye" )
DialogEnd()

end