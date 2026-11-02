import { CreateUserData, UpdateUserData, DeleteUserData, GetUserData, ListUsersData, CreateCelestialBodyData, UpdateCelestialBodyData, DeleteCelestialBodyData, GetCelestialBodyData, ListCelestialBodiesData, CreateEventData, UpdateEventData, DeleteEventData, GetEventData, ListEventsData, CreateSessionData, UpdateSessionData, DeleteSessionData, GetSessionData, ListMySessionsData, CreateObservationData, UpdateObservationData, DeleteObservationData, GetObservationData, ListMyObservationsData, CreatePhotoSettingData, UpdatePhotoSettingData, DeletePhotoSettingData, GetPhotoSettingData, ListMyPhotoSettingsData } from '../';
import { UseDataConnectQueryResult, useDataConnectQueryOptions, UseDataConnectMutationResult, useDataConnectMutationOptions} from '@tanstack-query-firebase/react/data-connect';
import { UseQueryResult, UseMutationResult} from '@tanstack/react-query';
import { DataConnect } from 'firebase/data-connect';
import { FirebaseError } from 'firebase/app';


export function useCreateUser(options?: useDataConnectMutationOptions<CreateUserData, FirebaseError, void>): UseDataConnectMutationResult<CreateUserData, undefined>;
export function useCreateUser(dc: DataConnect, options?: useDataConnectMutationOptions<CreateUserData, FirebaseError, void>): UseDataConnectMutationResult<CreateUserData, undefined>;

export function useUpdateUser(options?: useDataConnectMutationOptions<UpdateUserData, FirebaseError, void>): UseDataConnectMutationResult<UpdateUserData, undefined>;
export function useUpdateUser(dc: DataConnect, options?: useDataConnectMutationOptions<UpdateUserData, FirebaseError, void>): UseDataConnectMutationResult<UpdateUserData, undefined>;

export function useDeleteUser(options?: useDataConnectMutationOptions<DeleteUserData, FirebaseError, void>): UseDataConnectMutationResult<DeleteUserData, undefined>;
export function useDeleteUser(dc: DataConnect, options?: useDataConnectMutationOptions<DeleteUserData, FirebaseError, void>): UseDataConnectMutationResult<DeleteUserData, undefined>;

export function useGetUser(options?: useDataConnectQueryOptions<GetUserData>): UseDataConnectQueryResult<GetUserData, undefined>;
export function useGetUser(dc: DataConnect, options?: useDataConnectQueryOptions<GetUserData>): UseDataConnectQueryResult<GetUserData, undefined>;

export function useListUsers(options?: useDataConnectQueryOptions<ListUsersData>): UseDataConnectQueryResult<ListUsersData, undefined>;
export function useListUsers(dc: DataConnect, options?: useDataConnectQueryOptions<ListUsersData>): UseDataConnectQueryResult<ListUsersData, undefined>;

export function useCreateCelestialBody(options?: useDataConnectMutationOptions<CreateCelestialBodyData, FirebaseError, void>): UseDataConnectMutationResult<CreateCelestialBodyData, undefined>;
export function useCreateCelestialBody(dc: DataConnect, options?: useDataConnectMutationOptions<CreateCelestialBodyData, FirebaseError, void>): UseDataConnectMutationResult<CreateCelestialBodyData, undefined>;

export function useUpdateCelestialBody(options?: useDataConnectMutationOptions<UpdateCelestialBodyData, FirebaseError, void>): UseDataConnectMutationResult<UpdateCelestialBodyData, undefined>;
export function useUpdateCelestialBody(dc: DataConnect, options?: useDataConnectMutationOptions<UpdateCelestialBodyData, FirebaseError, void>): UseDataConnectMutationResult<UpdateCelestialBodyData, undefined>;

export function useDeleteCelestialBody(options?: useDataConnectMutationOptions<DeleteCelestialBodyData, FirebaseError, void>): UseDataConnectMutationResult<DeleteCelestialBodyData, undefined>;
export function useDeleteCelestialBody(dc: DataConnect, options?: useDataConnectMutationOptions<DeleteCelestialBodyData, FirebaseError, void>): UseDataConnectMutationResult<DeleteCelestialBodyData, undefined>;

export function useGetCelestialBody(options?: useDataConnectQueryOptions<GetCelestialBodyData>): UseDataConnectQueryResult<GetCelestialBodyData, undefined>;
export function useGetCelestialBody(dc: DataConnect, options?: useDataConnectQueryOptions<GetCelestialBodyData>): UseDataConnectQueryResult<GetCelestialBodyData, undefined>;

export function useListCelestialBodies(options?: useDataConnectQueryOptions<ListCelestialBodiesData>): UseDataConnectQueryResult<ListCelestialBodiesData, undefined>;
export function useListCelestialBodies(dc: DataConnect, options?: useDataConnectQueryOptions<ListCelestialBodiesData>): UseDataConnectQueryResult<ListCelestialBodiesData, undefined>;

export function useCreateEvent(options?: useDataConnectMutationOptions<CreateEventData, FirebaseError, void>): UseDataConnectMutationResult<CreateEventData, undefined>;
export function useCreateEvent(dc: DataConnect, options?: useDataConnectMutationOptions<CreateEventData, FirebaseError, void>): UseDataConnectMutationResult<CreateEventData, undefined>;

export function useUpdateEvent(options?: useDataConnectMutationOptions<UpdateEventData, FirebaseError, void>): UseDataConnectMutationResult<UpdateEventData, undefined>;
export function useUpdateEvent(dc: DataConnect, options?: useDataConnectMutationOptions<UpdateEventData, FirebaseError, void>): UseDataConnectMutationResult<UpdateEventData, undefined>;

export function useDeleteEvent(options?: useDataConnectMutationOptions<DeleteEventData, FirebaseError, void>): UseDataConnectMutationResult<DeleteEventData, undefined>;
export function useDeleteEvent(dc: DataConnect, options?: useDataConnectMutationOptions<DeleteEventData, FirebaseError, void>): UseDataConnectMutationResult<DeleteEventData, undefined>;

export function useGetEvent(options?: useDataConnectQueryOptions<GetEventData>): UseDataConnectQueryResult<GetEventData, undefined>;
export function useGetEvent(dc: DataConnect, options?: useDataConnectQueryOptions<GetEventData>): UseDataConnectQueryResult<GetEventData, undefined>;

export function useListEvents(options?: useDataConnectQueryOptions<ListEventsData>): UseDataConnectQueryResult<ListEventsData, undefined>;
export function useListEvents(dc: DataConnect, options?: useDataConnectQueryOptions<ListEventsData>): UseDataConnectQueryResult<ListEventsData, undefined>;

export function useCreateSession(options?: useDataConnectMutationOptions<CreateSessionData, FirebaseError, void>): UseDataConnectMutationResult<CreateSessionData, undefined>;
export function useCreateSession(dc: DataConnect, options?: useDataConnectMutationOptions<CreateSessionData, FirebaseError, void>): UseDataConnectMutationResult<CreateSessionData, undefined>;

export function useUpdateSession(options?: useDataConnectMutationOptions<UpdateSessionData, FirebaseError, void>): UseDataConnectMutationResult<UpdateSessionData, undefined>;
export function useUpdateSession(dc: DataConnect, options?: useDataConnectMutationOptions<UpdateSessionData, FirebaseError, void>): UseDataConnectMutationResult<UpdateSessionData, undefined>;

export function useDeleteSession(options?: useDataConnectMutationOptions<DeleteSessionData, FirebaseError, void>): UseDataConnectMutationResult<DeleteSessionData, undefined>;
export function useDeleteSession(dc: DataConnect, options?: useDataConnectMutationOptions<DeleteSessionData, FirebaseError, void>): UseDataConnectMutationResult<DeleteSessionData, undefined>;

export function useGetSession(options?: useDataConnectQueryOptions<GetSessionData>): UseDataConnectQueryResult<GetSessionData, undefined>;
export function useGetSession(dc: DataConnect, options?: useDataConnectQueryOptions<GetSessionData>): UseDataConnectQueryResult<GetSessionData, undefined>;

export function useListMySessions(options?: useDataConnectQueryOptions<ListMySessionsData>): UseDataConnectQueryResult<ListMySessionsData, undefined>;
export function useListMySessions(dc: DataConnect, options?: useDataConnectQueryOptions<ListMySessionsData>): UseDataConnectQueryResult<ListMySessionsData, undefined>;

export function useCreateObservation(options?: useDataConnectMutationOptions<CreateObservationData, FirebaseError, void>): UseDataConnectMutationResult<CreateObservationData, undefined>;
export function useCreateObservation(dc: DataConnect, options?: useDataConnectMutationOptions<CreateObservationData, FirebaseError, void>): UseDataConnectMutationResult<CreateObservationData, undefined>;

export function useUpdateObservation(options?: useDataConnectMutationOptions<UpdateObservationData, FirebaseError, void>): UseDataConnectMutationResult<UpdateObservationData, undefined>;
export function useUpdateObservation(dc: DataConnect, options?: useDataConnectMutationOptions<UpdateObservationData, FirebaseError, void>): UseDataConnectMutationResult<UpdateObservationData, undefined>;

export function useDeleteObservation(options?: useDataConnectMutationOptions<DeleteObservationData, FirebaseError, void>): UseDataConnectMutationResult<DeleteObservationData, undefined>;
export function useDeleteObservation(dc: DataConnect, options?: useDataConnectMutationOptions<DeleteObservationData, FirebaseError, void>): UseDataConnectMutationResult<DeleteObservationData, undefined>;

export function useGetObservation(options?: useDataConnectQueryOptions<GetObservationData>): UseDataConnectQueryResult<GetObservationData, undefined>;
export function useGetObservation(dc: DataConnect, options?: useDataConnectQueryOptions<GetObservationData>): UseDataConnectQueryResult<GetObservationData, undefined>;

export function useListMyObservations(options?: useDataConnectQueryOptions<ListMyObservationsData>): UseDataConnectQueryResult<ListMyObservationsData, undefined>;
export function useListMyObservations(dc: DataConnect, options?: useDataConnectQueryOptions<ListMyObservationsData>): UseDataConnectQueryResult<ListMyObservationsData, undefined>;

export function useCreatePhotoSetting(options?: useDataConnectMutationOptions<CreatePhotoSettingData, FirebaseError, void>): UseDataConnectMutationResult<CreatePhotoSettingData, undefined>;
export function useCreatePhotoSetting(dc: DataConnect, options?: useDataConnectMutationOptions<CreatePhotoSettingData, FirebaseError, void>): UseDataConnectMutationResult<CreatePhotoSettingData, undefined>;

export function useUpdatePhotoSetting(options?: useDataConnectMutationOptions<UpdatePhotoSettingData, FirebaseError, void>): UseDataConnectMutationResult<UpdatePhotoSettingData, undefined>;
export function useUpdatePhotoSetting(dc: DataConnect, options?: useDataConnectMutationOptions<UpdatePhotoSettingData, FirebaseError, void>): UseDataConnectMutationResult<UpdatePhotoSettingData, undefined>;

export function useDeletePhotoSetting(options?: useDataConnectMutationOptions<DeletePhotoSettingData, FirebaseError, void>): UseDataConnectMutationResult<DeletePhotoSettingData, undefined>;
export function useDeletePhotoSetting(dc: DataConnect, options?: useDataConnectMutationOptions<DeletePhotoSettingData, FirebaseError, void>): UseDataConnectMutationResult<DeletePhotoSettingData, undefined>;

export function useGetPhotoSetting(options?: useDataConnectQueryOptions<GetPhotoSettingData>): UseDataConnectQueryResult<GetPhotoSettingData, undefined>;
export function useGetPhotoSetting(dc: DataConnect, options?: useDataConnectQueryOptions<GetPhotoSettingData>): UseDataConnectQueryResult<GetPhotoSettingData, undefined>;

export function useListMyPhotoSettings(options?: useDataConnectQueryOptions<ListMyPhotoSettingsData>): UseDataConnectQueryResult<ListMyPhotoSettingsData, undefined>;
export function useListMyPhotoSettings(dc: DataConnect, options?: useDataConnectQueryOptions<ListMyPhotoSettingsData>): UseDataConnectQueryResult<ListMyPhotoSettingsData, undefined>;
