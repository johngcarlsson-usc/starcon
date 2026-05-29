-- TODO: everythink need to be converted to human comfortable reading form

UNTOUCHED = 0;
REJECTED  = 1;
ASSIGNED  = 2;
COMPLITED = 3;
FAILED    = 4;


quests = {
		{name = "ShofixtyQuest", file="gamedata/TestQuest.lua", status = UNTOUCHED },
		{name = "Ur-QuanQuest", file="gamedata/SecretPlanet.lua", status = UNTOUCHED }
	 }
-- when quest succesfuly complited,
-- index - index of complited quest
function QuestSuccess( index )
	quests[index].status = COMPLITED;
end

-- when quest failed,
-- index - index of complited quest
function QuestFailed( index )
	(quests[index]).status = FAILED;
end

-- when player ask for quest 
function GetNextQuest()
	if quests[1].status == UNTOUCHED then
		quests[1].status = ASSIGNED
		return "gamedata/TestQuest.lua";
	end
	return "NO_QUEST"
end

function GetQuest()
end

