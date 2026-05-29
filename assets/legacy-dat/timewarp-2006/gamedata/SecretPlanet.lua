-- Based on "SecretPlanet dialog for CTree" by youBastrd

Exist                      =   1;
matter_investigated        =   0;    -- if player have Visited Kohr-Ah station
next_remind_time           = 10000;    -- nex time player recive strange notification
first_strange_notification =   0;    -- player allready resived strange message
kzedrKilled                =   3;    -- Number Dreadnoughts to kill
Complited                  =   0;

station_x                  = 0;
station_y                  = 0;

function Process(time, enemy, kills, x, y, res_num1, res_num2, res_str)
	if time > next_remind_time then
		StrangeMessage();
		next_remind_time = next_remind_time + 100000;
	end;
end

function StrangeMessage()
	PrintMessage("XXXX...Hierarchy summon... piss");
	if first_strange_notification == 0 and matter_investigated == 0 then
		FirstDialog();
		first_strange_notification = 1;
	end;
end

function FirstDialog()
	DialogStart "gamedata/dialogs/human.jpg"
	DialogSetMusic "gamedata/dialogs/human.mod"
		DialogWrite "We have intercepted hyperwave radio transmissions using old Hierarchy frequencies..."
		DialogKeyPressed();
		DialogWrite "... which are coming from the planet's located in this region of space.  Please investigate the matter"
		DialogAnswer ("Aye Aye, sir.");
		DialogWrite "Be careful, this can be dangerous!"
		DialogKeyPressed();
	DialogEnd()
end;

function GAME_EVENT_SHIP_DIE( Type )
	if matter_investigated == 1 then
		if Type == "kzedr" then kzedrKilled = kzedrKilled - 1 end
		if kzedrKilled == 0 then Complited = 1; PrintMessage("Mission Completed."); end
	end
end

function GAME_EVENT_ENTER_STATION( location, x, y )
	station_x = x;
	station_y = y;
	if location == "Kohr-Ah" then
	
		if matter_investigated == 0 then
			-- Big Fun dialog
		DialogStart "gamedata/dialogs/spathi.jpg";
			DialogSetMusic "gamedata/dialogs/spathi.mod";
			function Question1() -- STARTCONVERSATION
				DialogWrite "What are you doing here? This planet is strictly off limits to -- I mean -- Hi, how would like to avoid killing me today?";
				local answer = DialogAnswer ("We heard your transmissions and thought you might be Ur-Quans",
											 "Nothing for now.  So long!"
											 );
				if answer == 1 then return Question2() end
				if answer == 2 then matter_investigated = -1; DialogWrite "Ok, so long!"; DialogKeyPressed(); return; end
			end

			function Question2() -- URQUAN
				DialogWrite "Ur-Quans?  Ha!  No way, no Ur-Quans here.  Those guys are way too intense for us."
				DialogKeyPressed();
				DialogWrite "Rather, we prefer a quiet, easy-going setting..."
				DialogKeyPressed();
				DialogWrite "which does not involve us getting painfully obliterated."

				local answer = DialogAnswer ("Are you sure?  There were transmissions coming near your location.",
											 "So long!  Keep your stick on the ice!" );
				if answer == 1 then return Question3() end
				if answer == 2 then matter_investigated = -1; DialogWrite "Ok, so long!"; DialogKeyPressed(); return; end
			end
			
			function Question3() -- URQUAN2
				DialogWrite "Ur-Quans?  Far from it, captain.  The transmissions were actually...";
				DialogKeyPressed();
				DialogWrite "...well this is kind of embarassing...";
				DialogKeyPressed();
				DialogWrite "homemade videos of myself I was sending to my spawning parter, Fwinda, on Spathiwa...";
				DialogKeyPressed();
				DialogWrite "who is what I believe you hunams call my mate.";
				local answer = DialogAnswer ("I guess not all Spathi are shy.", 
											 "So long!  Keep your stick on the ice!");
				if answer == 1 then return Question4() end
				if answer == 2 then matter_investigated = -1; DialogWrite "Ok, so long!"; DialogKeyPressed(); return end
			end
			
			function Question4() -- MAIN
				DialogWrite "Ahem.  Yes, well, where were we?";
				local answer = DialogAnswer ("Well, what are you doing here?",
											 "Have you seen any Ur-Quan around here?");
				if answer == 1 then return Question5() end
				if answer == 2 then return Question6() end
			end
			
			function Question5() -- DOING1
				DialogWrite "As I'm sure you know, this planet was uninhabited for quite some time.";
				DialogKeyPressed();
				DialogWrite "And the last time we spoke to your species, was just before we...";
				DialogKeyPressed();
				DialogWrite "wisely evaded the rest of the universe under our self-made slave shield.";
				DialogKeyPressed();
				DialogWrite "But a few of us brave Spathi grew tired of being under the shield,";
				DialogKeyPressed();
				DialogWrite "and decided to get our extremeties wet in the real universe.";
			
				local answer = DialogAnswer ("Are you kidding?  Brave Spathi?",
											 "Let's talk about something else.",
											 "So long!  Keep your stick on the ice!");
				if answer == 1 then return Question7() end
				if answer == 2 then return Question8() end
				if answer == 3 then matter_investigated = -1; DialogWrite "Ok, so long!"; DialogKeyPressed(); return end
			end

			function Question6() -- MAIN (from second position)
				DialogWrite "Nope, not a one!  You can normally tell where there is Ur-Quan activity.";
				DialogKeyPressed();
				DialogWrite "Their presence is usually marked with a lack of any other kind of activity...";
				DialogKeyPressed();
				DialogWrite "except for smoking remains.";
			
				local answer = DialogAnswer ("Well, what are you doing here?",
											 "Have you seen any Ur-Quan around here?");
				if answer == 1 then return Question5() end
				if answer == 2 then return Question6() end
			end
			
			function Question7() -- DOING2 (from 3 position)
				DialogWrite "Oh, for sure!  You know, just because we multiply in the tens of thousands,";
				DialogKeyPressed();
				DialogWrite "as there's safety in numbers,";
				DialogKeyPressed();
				DialogWrite "and just because any of our species will choose to rather run from any fight,";
				DialogKeyPressed();
				DialogWrite "with our appendages flailing in the air,"
				DialogKeyPressed();
				DialogWrite "doesn't mean that there's no brave Spathi."
				DialogKeyPressed();
				DialogWrite "The brave few of us just stand out of the crowd a little more, that's all."

				local answer = DialogAnswer ("So what kind of things have you been doing here?",
											 "How did you get past the slave shield around your homeworld?",
											 "Let's talk about something else.");
				if answer == 1 then return Question9() end
				if answer == 2 then return Question10() end
				if answer == 3 then return Question8() end
			end;
			
			function Question8() -- MAIN (from 3,4 position)
				DialogWrite "Sure, let's talk about something else.";
			
				local answer = DialogAnswer ("Well, what are you doing here?",
											 "Have you seen any Ur-Quan around here?");
				if answer == 1 then return Question5() end
				if answer == 2 then return Question6() end
			end

			function Question9() -- DOING3A
				DialogWrite "Oh, not much, just kicking around on this planet,";
				DialogKeyPressed();
				DialogWrite "taking a break from being terrified of everything on Spathiwa,";
				DialogKeyPressed();
				DialogWrite "by being terrified by everything here."
				local answer = DialogAnswer ("That doesn't sound too believable, you came all this way to do nothing?",
											"How did you get past the slave shield around your homeworld?",
											"Let's talk about something else.",
											"So long!  Keep your stick on the ice!");
				if answer == 1 then return Question11() end
				if answer == 2 then return Question10() end
				if answer == 3 then return Question8() end
				if answer == 4 then matter_investigated = -1; DialogWrite "Ok, so long!"; DialogKeyPressed(); return end
			end
			function Question10() -- DOING3B
				DialogWrite "That's a very good question, with a very good answer.";
				DialogKeyPressed();
				DialogWrite "We just, um, went around it.";
				local answer = DialogAnswer ("So what kind of things have you been doing here?",
											 "You... went around it?  How?  It goes around the entire planet!",
											 "Let's talk about something else.",
											 "So long!  Keep your stick on the ice!");

				if answer == 1 then return Question9() end
				if answer == 2 then return Question12() end
				if answer == 3 then return Question8() end
				if answer == 4 then matter_investigated = -1; DialogWrite "Ok, so long!"; DialogKeyPressed(); return end
			end
			
			function Question11() -- LIEING1
				DialogWrite "Well we came here, to, um see how the stars look from way over here.";
				local answer = DialogAnswer ("Somehow, I just don't believe you.  What are you covering up?");
				if answer == 1 then return Question13() end;
			end
			function Question12() -- LIENG1(2)
				DialogWrite "Near the polar regions, there's, um, weaker spots in the shield.";
				DialogKeyPressed();
				DialogWrite "We just took a lander ship, bounced around a little until it wanted to go through,";
				DialogKeyPressed();
				DialogWrite "and saw open space for the first time in years!"
				DialogKeyPressed();
				DialogWrite "Then we, um, flew out here... in the lander... and laned on this planet.  For no real reason.";
				local answer = DialogAnswer ("Somehow, I just don't believe you.  What are you covering up?");
				if answer == 1 then return Question13() end;				
			end
			function Question13() -- LEING2
				DialogWrite "I can't take it anymore!  I can't stand lieing to you, hunam!";
				DialogSetMusic "gamedata/dialogs/urquan.mod";
				DialogKeyPressed();
				DialogWrite "We've been taken captive by the Ur-Quan!";
				DialogKeyPressed();
				DialogWrite "They're doing things to ... down here, trying to change us ... soldiers ...";
				DialogKeyPressed();
				DialogWrite "Oh no!  .... are jamming ... can't ...";
				DialogKeyPressed();
				DialogSetAlienImage "gamedata/dialogs/urquan.jpg"
				DialogWrite "This transmission cannot be allowed.";
				DialogKeyPressed();
				DialogWrite "You are not welcome here.";
				DialogKeyPressed();
				DialogWrite "These creatures are ours, now."
				local answer = DialogAnswer ("You bastrd!  What have you been doing here?",
											 "Hey, slimeball, didn't I waste you years ago?",
											 "Ever think of joining forces with us?",
											 "You are so dead, pal!",
											 "Well, look at the time.  Gotta run!")

				if answer == 1 then return Question14() end
				if answer == 2 then return Question15() end
				if answer == 3 then return Question16() end
				if answer == 4 then return Question17() end
				if answer == 5 then 
					DialogWrite "Not so easy, human. Time to die."; 
					DialogKeyPressed(); CreateFight(); 
					return;
				end
			end
			
			function Question14() -- INTENTIONS
				DialogWrite "That is not your concern."
				DialogKeyPressed();
				DialogWrite "You have interfered with our plans all too often.";
				DialogKeyPressed();
				DialogWrite "Our intentions with these subordinates is only our concern."
				
				DialogAnswer ("The Spathi are no longer your subordinates.");
				
				DialogWrite "The Spathi on the surface were taken from their homeworld and brought here.";
				DialogKeyPressed();
				DialogWrite "If they had completed their simple task of remaining silent,";
				DialogKeyPressed();
				DialogWrite "and warned all who came near to leave,";
				DialogKeyPressed();
				DialogWrite "then we would not have this conflict.";
				DialogKeyPressed();
				DialogSetAlienImage "gamedata/dialogs/spathi.jpg";
				DialogWrite "I'm swtiching to another frequency, can you hear me hunam?";
				DialogKeyPressed();
				DialogWrite "Please don't let me die!  Tomorrow would be much better!";
				DialogKeyPressed();
				DialogSetAlienImage "gamedata/dialogs/urquan.jpg";
				DialogWrite "Be silent, Spathi.";
				DialogKeyPressed();
				DialogWrite "Your death will come soon enough.";
				DialogKeyPressed();
				DialogWrite "Your species' choice to be hidden under the slave shield would protect your species from harm,"
				DialogKeyPressed();
				DialogWrite "but you forgot that it was we who used the slave shield on countless planets...";
				DialogKeyPressed();
				DialogWrite "and we who could bypass it.";
				DialogKeyPressed();
				DialogWrite "While it protected you from harm from outsiders,"
				DialogKeyPressed();
				DialogWrite "It also prevented the scans from your allies above.";
				DialogKeyPressed();
				DialogWrite "This made your Spathiwa the perfect planet";
				DialogKeyPressed();
				DialogWrite "on which to train a vast army."
				DialogKeyPressed();
				DialogSetAlienImage "gamedata/dialogs/spathi.jpg";
				DialogWrite "Nooo!  Fwinda!"
				DialogKeyPressed();
				DialogWrite "I'll probably lose my pr0n collection too!";
				DialogKeyPressed();
				DialogSetAlienImage "gamedata/dialogs/urquan.jpg";
				DialogAnswer ("Hey creepy-looking slimey thing, um, the evil one of the two: you forgot that the Spathi are wimps!");
				DialogWrite "While this is true,";
				DialogKeyPressed();
				DialogWrite "our methods of training can turn any species into formitable warriors.";
				DialogKeyPressed();
				DialogWrite "By bringing these few Spathi here, we can determine if their species can be changed.";
				DialogKeyPressed();
				DialogWrite "Should we be successful, we will breed these soldiers in vast numbers.";
				DialogKeyPressed();
				DialogWrite "Be warned, foolish human, you cannot resist us forever.";
				DialogKeyPressed();
				DialogWrite "You will submit, or die!";
				DialogKeyPressed();
				local answer = DialogAnswer ("You are so dead, pal!",
											 "Well, look at the time.  Gotta run!");
				if answer == 1 then return Question17() end
				if answer == 2 then DialogWrite("You know too much, human."); DialogKeyPressed(); return Question17() end
			end
			
			function Question15() -- DIALOG (2)
				DialogWrite "We, the Ur-Quan Kzer-za, are strong.";
				DialogKeyPressed();
				DialogWrite "We are not so easily defeated.";
				DialogKeyPressed();
				DialogWrite "We will be restored to our former stature as overlords of many species...";
				DialogKeyPressed();
				DialogWrite "as it has been for millenia."
				
				local answer = DialogAnswer ("You bastrd!  What have you been doing here?",
											 "Hey, slimeball, didn't I waste you years ago?",
											 "Ever think of joining forces with us?",
											 "You are so dead, pal!",
											 "Well, look at the time.  Gotta run!")

				if answer == 1 then return Question14() end
				if answer == 2 then return Question15() end
				if answer == 3 then return Question16() end
				if answer == 4 then return Question17() end
				if answer == 5 then 
					DialogWrite "Not so easy, human. Time to die."; 
					DialogKeyPressed(); CreateFight(); 
					return;
				end
			end
			
			function Question16() -- DIALOG (3)
				DialogWrite "Your pitiful attempt to ... yadda yadda ..."
				local answer = DialogAnswer ("You bastrd!  What have you been doing here?",
											 "Hey, slimeball, didn't I waste you years ago?",
											 "Ever think of joining forces with us?",
											 "You are so dead, pal!",
											 "Well, look at the time.  Gotta run!")

				if answer == 1 then return Question14() end
				if answer == 2 then return Question15() end
				if answer == 3 then return Question16() end
				if answer == 4 then return Question17() end
				if answer == 5 then 
					DialogWrite "Not so easy, human. Time to die."; 
					DialogKeyPressed(); CreateFight(); 
					return;
				end
			end
			
			function Question17() -- COMBAT
				DialogWrite "Die, human";
				DialogKeyPressed();
				CreateFight();
			end
				
			Question1();
		DialogEnd ();
		
		matter_investigated = matter_investigated + 1;
		return;
		end
		if Complited == 0 then
			-- Ur-quan boasting
		DialogStart "gamedata/dialogs/urquan.jpg"
			DialogSetMusic "gamedata/dialogs/urquan.mod";
			DialogWrite ("We will destroy you our ships already on they way!!!");
			DialogAnswer ("Ha");
		DialogEnd();
		end 
		if Complited == 1 and Exist == 1 then
		-- Dialog about capturing kohr-ah factory
		DialogStart "gamedata/dialogs/spathi.jpg"
			DialogSetMusic "gamedata/dialogs/spathi.mod";
			DialogWrite ("Savior!!! We have Ur-quan ship factory here and now we leave it to you. It can produce Kohr-Ah Marauders!!!");
			DialogKeyPressed();
			DialogWrite ("We are off to Spathiwa, we will send you bill for factory from there");
			DialogAnswer ("Uuuu");
		DialogEnd();
		MakeAlly(station_x, station_y);
		Exist = 0;
		return;
		end	
	end
end


function CreateFight()
AddEnemyShip("kzedr", station_x, station_y);
AddEnemyShip("kzedr", station_x, station_y);
AddEnemyShip("kzedr", station_x, station_y);
end



