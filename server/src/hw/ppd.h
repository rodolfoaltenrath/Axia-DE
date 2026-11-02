/* ppd.h — power profiles for Settings → Battery: power-profiles-daemon over the system bus when
 * it runs (org.freedesktop.UPower.PowerProfiles, or the older net.hadess.PowerProfiles), else
 * the cpufreq governor read from sysfs. Changing it as the user goes through the daemon (its
 * polkit policy allows active sessions); if that is refused the theme falls back to the closed
 * admin action power-profile (hw/hw_admin.c). */
#ifndef LORD_HW_PPD_H
#define LORD_HW_PPD_H

typedef void (*PpdEvent)(const char *kind, const char *json);

void ppd_start(PpdEvent fn);            /* after sysbus_start (works without it: sysfs only) */
char *ppd_state_json(void);             /* {daemon, active, profiles:[...], degraded, governor} */
/* "performance", "balanced" or "power-saver". The result comes as "ppd-result" {ok, error}. */
int  ppd_set(const char *profile);
int  ppd_profile_valid(const char *p);

#endif
