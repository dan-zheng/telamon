initSidebarItems({"enum":[["DeadEndSource",""],["LogMessage",""],["TreeEvent","The possible tree events. WARNING:  Changing the enums will break any pre-existing eventlog files.  Adding new cases at the end only is safe."]],"fn":[["find_best","Entry point of the exploration. This function returns the best candidate that it has found in the given time (or at whatever point we decided to stop the search - potentially after an exhaustive search)"],["find_best_ex","Same as `find_best`, but allows to specify pre-existing actions and also returns the actionsfor the best candidate."],["gen_space","Explores the full search space."]],"mod":[["choice","Choices that can be applied to split the search space."],["config","Defines a structure to store the configuration of the exploration. The configuration is read from the `Setting.toml` file if it exists. Some parameters can be overridden from the command line."],["local_selection","Provides different methods to select a candidate in a list."]],"struct":[["Candidate","A node of the search tree."]]});