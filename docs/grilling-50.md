# Grilling session — 66 questions

Answer out loud, in order, without notes. "I don't know" is a valid answer; a confident wrong number is
not. Questions are grouped so a bad answer early does not poison the rest.

If you only do five: **12, 21, 38, 47, 55.**

---

## A. Positioning — what is this, exactly?

1. In one sentence, what does this app do that `npx skills` and a browser tab do not?
2. Who is the person that opens it twice a week without being reminded?
3. What did that person do before this existed, and how long did it take?
4. Name the competitor you are most afraid of, and the one you are least afraid of.
5. If Claude Code shipped a built-in skill browser tomorrow, what survives?
6. Is this a marketplace, a package manager, a safety scanner or a search engine? Pick one.
7. What is the honest answer to "why not just use GitHub search"?
8. What would make you kill this project?

## B. Discovery and coverage — the ask that started this

9. You asked for GitHub, SkillsMP and SkillHub. Which of the three would you keep if one had to go?
10. SkillsMP indexes 3M+ skills; this machine has 2,737. Is the product "find more" or "know what I have"?
11. When the same skill appears in three sources, which one is canonical, and who decides?
12. What happens when a source breaks — do skills disappear from the list, or go stale with a date?
13. What is the acceptable freshness for a listing? A week? A day?
14. Do you need full bodies indexed, or is metadata plus a lazy fetch enough?
15. What is the largest index size you are willing to ship, in MB?
16. Should discovery work offline? If yes, for how long after the last sync?
17. What is the search quality bar — exact name, fuzzy name, or "describe the problem"?
18. Is a live marketplace query allowed on every keystroke, or is it a deliberate action?

## C. Clones, duplicates and ranking

19. Two skills are 94% similar. Do you show both, merge them, or hide the loser?
20. Who is allowed to call a variant "worse" — a score, a date, a human?
21. If the older fork is better maintained, does age or stars win?
22. What does "canonical" mean when the upstream is abandoned?
23. Can a curated pick ever be a fork? What is the policy?
24. How do you detect a copy that has been reformatted by an LLM? Is SimHash enough?
25. What is the false-merge cost — worst case, someone installs the wrong skill. How bad is that?
26. When ranking, does safety outrank relevance? Always?

## D. Safety grading

27. What does a grade of B actually promise a user?
28. Which is worse: a skill that reads `~/.aws/credentials`, or one that declares it does?
29. Can a grade ever go up without a human reading the diff?
30. What is your liability wording when a graded-A skill exfiltrates data?
31. Do you scan only `SKILL.md`, or every file? What does the UI say when it is only one file?
32. How do you grade a skill that tells the agent to fetch and run a script at runtime?
33. Is the grade per-file or per-version? What happens on a new commit?
34. Would you ever block an install, or only warn?

## E. Dependencies — the new tab

35. Why does a skill catalogue need a dependency tab at all?
36. Who decides that a skill "requires" ripgrep — the manifest, the scan, or the user?
37. If the app can install binaries, what stops it from becoming a second package manager?
38. What is the review step before executing `winget install`? Show the command, or just do it?
39. Do you track versions, or only presence on PATH?
40. How do you handle a dependency that needs a restart of the shell to appear?
41. What happens when the install command fails halfway?
42. Do dependencies get their own safety grade, or inherit the skill's?
43. Is a missing dependency a warning, a blocker, or an automatic install?

## F. Terminals and environments

44. Why more than one terminal? What task actually breaks with a single shell?
45. Does a skill run in the user's shell, a sandbox, a container, or WSL?
46. If WSL is installed, is the index shared between Windows and Linux paths?
47. What is the security boundary when the agent can open a terminal it chooses?
48. Do you allow SSH targets in v1? What changes if you do?
49. How does the app behave when a command is interactive and waits for input?
50. What is logged from those terminals, and where does the log live?

## G. Persistence and the agent surface

51. Why SQLite and not JSON files? At what row count does that stop being true?
52. What is stored per skill: description only, or body too? Where do bodies live?
53. Can the index be rebuilt from scratch in under a minute? How do you know?
54. What does "inquire in text" return — prose, or rows?
55. If an agent asks "is there a skill for OCR?", what is the exact output shape?
56. How does an agent download a skill — a CLI verb, a file path, an HTTP call?
57. Is the database readable by other tools while the app is running? WAL or lock?
58. What happens to the index when a skill is uninstalled outside the app?

## H. Business and durability

59. Would you pay $10/month for this? Answer as the user, not the builder.
60. What is the free tier, and what is behind the paywall?
61. If every marketplace blocked you tomorrow, what is left?
62. What is the smallest version you would put in front of a stranger this week?
63. Which number on screen would make you distrust the whole product if it were wrong?
64. What is the maintenance cost per month of keeping the index fresh?
65. Who reviews the curated seed list, and how often?
66. What does success look like in six months — installs, retention, or a metric you can measure alone?

---

## Follow-ups to ask after each answer

- What evidence do you have for that, and how old is it?
- What would change your mind?
- What is the cheapest experiment that could falsify it?
- Who else has to agree for this to ship?
- What breaks first at 10× the volume?
