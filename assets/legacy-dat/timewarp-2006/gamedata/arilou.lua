-- Default dialog screen


function DIALOG ()
DialogStart "gamedata/dialogs/arilou.jpg"
	DialogSetMusic "gamedata/dialogs/arilou.mod"
	DialogWrite "Hello my little one. I am so pleased to see you! You have done well for yourself. It is gratifying."
	answer = DialogAnswer ( "You sound as if you know me. Have we met?",
				"I know who you are! You're Arilou!! We've wondered what happened to your people for a long time.",
				"I'll be. It's the Arilou. Why the hell did you run out on the Alliance of Free Stars? What happened?" )
DialogEnd()

end