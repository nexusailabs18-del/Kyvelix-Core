package main

import (
	"fmt"
	"sync"
)

var profileList = []string{
	"chrome_120",
	"chrome_124",
	"chrome_131",
	"chrome_133",
	"chrome_150",
	"firefox_126",
}

var currentProfileIndex = 0
var profileMutex sync.Mutex

func getNextProfile() string {
	profileMutex.Lock()
	defer profileMutex.Unlock()

	profile := profileList[currentProfileIndex]
	currentProfileIndex = (currentProfileIndex + 1) % len(profileList)
	return profile
}

func rotateProfileAfterBlock() string {
	return getNextProfile()
}

func getAllProfiles() []string {
	return profileList
}

func printProfileStats() {
	profileMutex.Lock()
	defer profileMutex.Unlock()
	fmt.Printf("Current profile index: %d/%d\n", currentProfileIndex, len(profileList))
	fmt.Println("Available profiles:", profileList)
}
